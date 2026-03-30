use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use regex::Regex;
use reqwest::header::{FROM, HeaderMap, HeaderValue, RETRY_AFTER};
use reqwest::{Client, Response, StatusCode};
use roxmltree::{Document, Node};
use serde::Serialize;
use tokio::time::{Duration, sleep};

const API_BASE_URL: &str = "https://export.arxiv.org/api/query";
const PDF_BASE_URL: &str = "https://arxiv.org/pdf";
const SOURCE_BASE_URL: &str = "https://export.arxiv.org/e-print";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const MAX_RETRIES: usize = 6;
const RETRY_DELAY_MS: u64 = 3_000;
// arxiv.org/robots.txt currently advertises Crawl-delay: 15 for User-agent: *.
const ROBOTS_CRAWL_DELAY_MS: u64 = 15_000;
const MIN_RATE_LIMIT_DELAY_MS: u64 = ROBOTS_CRAWL_DELAY_MS;
const MAX_RETRY_DELAY_MS: u64 = 60_000;
const API_WINDOW_LIMIT: usize = 30_000;
const MAX_PAGE_SIZE: usize = 2_000;

#[derive(Debug, Clone, Copy)]
pub enum DownloadFormat {
    Pdf,
    Source,
}

impl DownloadFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Source => "tar.gz",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SortBy {
    Relevance,
    LastUpdatedDate,
    SubmittedDate,
}

impl SortBy {
    pub fn as_api_value(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::LastUpdatedDate => "lastUpdatedDate",
            Self::SubmittedDate => "submittedDate",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    pub fn as_api_value(self) -> &'static str {
        match self {
            Self::Ascending => "ascending",
            Self::Descending => "descending",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub raw_query: Option<String>,
    pub all_terms: Vec<String>,
    pub title_terms: Vec<String>,
    pub author_terms: Vec<String>,
    pub category_terms: Vec<String>,
    pub abstract_terms: Vec<String>,
    pub id_list: Vec<String>,
    pub start: usize,
    pub max_results: usize,
    pub sort_by: SortBy,
    pub sort_order: SortOrder,
}

impl SearchRequest {
    pub fn to_url_with_base(&self, api_base_url: &str) -> Result<String> {
        let mut params = Vec::new();

        if let Some(query) = self.build_search_query()? {
            params.push(format!("search_query={}", urlencoding::encode(&query)));
        }

        if !self.id_list.is_empty() {
            params.push(format!(
                "id_list={}",
                urlencoding::encode(&self.id_list.join(","))
            ));
        }

        params.push(format!("start={}", self.start));
        params.push(format!("max_results={}", self.max_results));
        params.push(format!("sortBy={}", self.sort_by.as_api_value()));
        params.push(format!("sortOrder={}", self.sort_order.as_api_value()));

        Ok(format!("{api_base_url}?{}", params.join("&")))
    }

    fn build_search_query(&self) -> Result<Option<String>> {
        if let Some(raw_query) = self.raw_query.as_ref() {
            let raw_query = raw_query.trim();
            if raw_query.is_empty() {
                bail!("--raw-query 不能为空");
            }
            return Ok(Some(raw_query.to_string()));
        }

        let mut clauses = Vec::new();
        append_terms(&mut clauses, "all", &self.all_terms);
        append_terms(&mut clauses, "ti", &self.title_terms);
        append_terms(&mut clauses, "au", &self.author_terms);
        append_terms(&mut clauses, "cat", &self.category_terms);
        append_terms(&mut clauses, "abs", &self.abstract_terms);

        if clauses.is_empty() {
            if self.id_list.is_empty() {
                bail!(
                    "至少提供一个查询条件：--raw-query / --query / --title / --author / --category / --abstract-text / --id"
                );
            }
            Ok(None)
        } else {
            Ok(Some(clauses.join(" AND ")))
        }
    }
}

fn append_terms(target: &mut Vec<String>, prefix: &str, terms: &[String]) {
    for term in terms {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }
        target.push(format!("{prefix}:{}", quote_term(term)));
    }
}

fn quote_term(term: &str) -> String {
    if term.contains(char::is_whitespace) || term.contains(':') || term.contains('"') {
        let escaped = term.replace('"', r#"\""#);
        format!("\"{escaped}\"")
    } else {
        term.to_string()
    }
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub total_results: usize,
    pub start_index: usize,
    pub items_per_page: usize,
    pub entries: Vec<Paper>,
}

#[derive(Debug, Serialize)]
pub struct Paper {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub summary: String,
    pub published: Option<String>,
    pub updated: Option<String>,
    pub authors: Vec<String>,
    pub categories: Vec<String>,
    pub primary_category: Option<String>,
    pub pdf_url: Option<String>,
    pub doi: Option<String>,
    pub comment: Option<String>,
    pub journal_ref: Option<String>,
}

pub struct ArxivClient {
    http: Client,
    api_base_url: String,
    pdf_base_url: String,
    source_base_url: String,
}

impl ArxivClient {
    pub fn new() -> Result<Self> {
        let contact_email = env::var("ARXIV_CONTACT_EMAIL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let mut builder = Client::builder();
        if let Some(email) = contact_email.as_deref() {
            let mut headers = HeaderMap::new();
            if let Ok(from_header) = HeaderValue::from_str(email) {
                headers.insert(FROM, from_header);
            }
            builder = builder
                .default_headers(headers)
                .user_agent(build_user_agent(Some(email)));
        } else {
            builder = builder.user_agent(build_user_agent(None));
        }

        let http = builder.build().context("创建 HTTP 客户端失败")?;
        Ok(Self {
            http,
            api_base_url: env::var("ARXIV_API_BASE_URL")
                .unwrap_or_else(|_| API_BASE_URL.to_string()),
            pdf_base_url: env::var("ARXIV_PDF_BASE_URL")
                .unwrap_or_else(|_| PDF_BASE_URL.to_string()),
            source_base_url: env::var("ARXIV_SOURCE_BASE_URL")
                .unwrap_or_else(|_| SOURCE_BASE_URL.to_string()),
        })
    }

    pub async fn search(&self, request: &SearchRequest) -> Result<SearchResponse> {
        let url = request.to_url_with_base(&self.api_base_url)?;
        let response = self.send_with_retries(&url).await?;

        let body = response.text().await.context("读取 arXiv API 响应失败")?;
        parse_feed(&body)
    }

    pub async fn download(
        &self,
        input: &str,
        format: DownloadFormat,
        output: Option<&Path>,
        overwrite: bool,
    ) -> Result<PathBuf> {
        let paper_id = normalize_paper_id(input)?;
        let url = self.download_url(&paper_id, format);

        let target = match output {
            Some(path) => path.to_path_buf(),
            None => default_output_path(&paper_id, format),
        };

        if target.exists() && !overwrite {
            bail!("目标文件已存在: {}，如需覆盖请加 --force", target.display());
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建输出目录失败: {}", parent.display()))?;
        }

        let bytes = self
            .send_with_retries(&url)
            .await?
            .bytes()
            .await
            .context("读取下载内容失败")?;

        std::fs::write(&target, &bytes)
            .with_context(|| format!("写入文件失败: {}", target.display()))?;

        Ok(target)
    }

    pub async fn search_all(
        &self,
        request: &SearchRequest,
        limit: Option<usize>,
        batch_size: usize,
    ) -> Result<SearchResponse> {
        if batch_size == 0 {
            bail!("batch_size 必须大于 0");
        }
        if request.start >= API_WINDOW_LIMIT {
            bail!("start 超出 arXiv API 窗口限制({API_WINDOW_LIMIT})，请减小 --start");
        }

        let window_remaining = API_WINDOW_LIMIT.saturating_sub(request.start);
        let mut remaining = limit.unwrap_or(window_remaining).min(window_remaining);

        let mut all_entries = Vec::new();
        let mut start = request.start;
        let mut total_results = 0usize;

        while remaining > 0 {
            let mut paged = request.clone();
            paged.start = start;
            paged.max_results = remaining.min(batch_size).min(MAX_PAGE_SIZE);

            let page = self.search(&paged).await?;
            if total_results == 0 {
                total_results = page.total_results;
            }

            if page.entries.is_empty() {
                break;
            }

            let fetched = page.entries.len();
            all_entries.extend(page.entries);
            start = start.saturating_add(fetched);
            remaining = remaining.saturating_sub(fetched);

            if fetched < paged.max_results || start >= total_results || start >= API_WINDOW_LIMIT {
                break;
            }
        }

        Ok(SearchResponse {
            total_results,
            start_index: request.start,
            items_per_page: all_entries.len(),
            entries: all_entries,
        })
    }

    fn download_url(&self, paper_id: &str, format: DownloadFormat) -> String {
        match format {
            DownloadFormat::Pdf => format!("{}/{paper_id}.pdf", self.pdf_base_url),
            DownloadFormat::Source => format!("{}/{paper_id}", self.source_base_url),
        }
    }

    async fn send_with_retries(&self, url: &str) -> Result<Response> {
        for attempt in 0..=MAX_RETRIES {
            let response = self
                .http
                .get(url)
                .send()
                .await
                .with_context(|| format!("请求失败: {url}"))?;

            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }

            let retry_after = parse_retry_after(response.headers());
            if attempt < MAX_RETRIES && should_retry(status) {
                let delay = compute_retry_delay(status, attempt, retry_after);
                sleep(delay).await;
                continue;
            }

            let body = response.text().await.unwrap_or_default();
            let detail = compact_error_body(&body);
            match detail {
                Some(detail) => {
                    bail!("请求失败: {url} (status {}) - {detail}", status.as_u16());
                }
                None if status == StatusCode::TOO_MANY_REQUESTS => {
                    bail!(
                        "请求被 arXiv 限流: {url} (status 429)。根据 arxiv.org/robots.txt 建议，至少每 15 秒发起一次请求；可设置 ARXIV_CONTACT_EMAIL 提升可识别性"
                    );
                }
                None => {
                    bail!("请求失败: {url} (status {})", status.as_u16());
                }
            }
        }

        bail!("请求失败: {url}")
    }
}

fn build_user_agent(contact_email: Option<&str>) -> String {
    match contact_email {
        Some(email) => format!("{USER_AGENT} (mailto:{email})"),
        None => USER_AGENT.to_string(),
    }
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    let seconds = value.parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}

fn compute_retry_delay(
    status: StatusCode,
    attempt: usize,
    retry_after: Option<Duration>,
) -> Duration {
    let backoff_ms = RETRY_DELAY_MS
        .saturating_mul(1_u64 << attempt.min(4))
        .min(MAX_RETRY_DELAY_MS);
    let base_delay = Duration::from_millis(backoff_ms);
    let from_header = retry_after.unwrap_or(base_delay);

    if status == StatusCode::TOO_MANY_REQUESTS {
        from_header.max(Duration::from_millis(MIN_RATE_LIMIT_DELAY_MS))
    } else {
        from_header
    }
}

fn should_retry(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn compact_error_body(body: &str) -> Option<String> {
    let normalized = normalize_text(body);
    if normalized.is_empty() {
        None
    } else {
        Some(summarize_text(&normalized, 200))
    }
}

fn default_output_path(paper_id: &str, format: DownloadFormat) -> PathBuf {
    let safe_name = paper_id.replace('/', "_");
    PathBuf::from(format!("{safe_name}.{}", format.extension()))
}

fn parse_feed(xml: &str) -> Result<SearchResponse> {
    let document = Document::parse(xml).context("解析 Atom XML 失败")?;
    let feed = document.root_element();

    let total_results = feed_child_text(&feed, "totalResults")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    let start_index = feed_child_text(&feed, "startIndex")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    let items_per_page = feed_child_text(&feed, "itemsPerPage")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();

    let entries = feed
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "entry")
        .map(parse_entry)
        .collect::<Result<Vec<_>>>()?;

    Ok(SearchResponse {
        total_results,
        start_index,
        items_per_page,
        entries,
    })
}

fn parse_entry(entry: Node<'_, '_>) -> Result<Paper> {
    let id = child_text(entry, "id").unwrap_or_default();
    let short_id = short_id_from_entry_id(&id);
    let title = normalize_text(&child_text(entry, "title").unwrap_or_default());
    let summary = normalize_text(&child_text(entry, "summary").unwrap_or_default());
    let published = child_text(entry, "published");
    let updated = child_text(entry, "updated");

    let authors = entry
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "author")
        .filter_map(|author| child_text(author, "name"))
        .map(|name| normalize_text(&name))
        .collect::<Vec<_>>();

    let mut categories = entry
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "category")
        .filter_map(|node| node.attribute("term"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    categories.sort();
    categories.dedup();

    let primary_category = entry
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "primary_category")
        .and_then(|node| node.attribute("term"))
        .map(ToOwned::to_owned)
        .or_else(|| categories.first().cloned());

    let pdf_url = entry
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "link")
        .find_map(|node| {
            let href = node.attribute("href")?;
            let title = node.attribute("title");
            if matches!(title, Some("pdf")) || href.contains("/pdf/") {
                Some(href.to_string())
            } else {
                None
            }
        });

    Ok(Paper {
        id,
        short_id,
        title,
        summary,
        published,
        updated,
        authors,
        categories,
        primary_category,
        pdf_url,
        doi: child_text(entry, "doi"),
        comment: child_text(entry, "comment"),
        journal_ref: child_text(entry, "journal_ref"),
    })
}

fn feed_child_text(feed: &Node<'_, '_>, name: &str) -> Option<String> {
    feed.children()
        .find(|node| node.is_element() && node.tag_name().name() == name)
        .and_then(|node| node.text())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn child_text(node: Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
        .and_then(|child| child.text())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn summarize_text(text: &str, max_len: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_len {
        return text.to_string();
    }

    let truncated = text
        .chars()
        .take(max_len.saturating_sub(1))
        .collect::<String>();
    format!("{}…", truncated.trim_end())
}

fn short_id_from_entry_id(entry_id: &str) -> String {
    entry_id
        .rsplit("/abs/")
        .next()
        .unwrap_or(entry_id)
        .trim_matches('/')
        .to_string()
}

pub fn normalize_paper_id(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("论文 ID 不能为空");
    }

    let trimmed = trimmed.strip_prefix("arXiv:").unwrap_or(trimmed);

    if let Some(rest) = trimmed.split("/abs/").nth(1) {
        return sanitize_id(rest);
    }
    if let Some(rest) = trimmed.split("/pdf/").nth(1) {
        return sanitize_id(rest);
    }

    let old_style = Regex::new(r"^[a-z\-]+(?:\.[A-Z]{2})?/\d{7}(v\d+)?$")
        .map_err(|err| anyhow!("创建旧式 ID 正则失败: {err}"))?;
    let new_style = Regex::new(r"^\d{4}\.\d{4,5}(v\d+)?$")
        .map_err(|err| anyhow!("创建新式 ID 正则失败: {err}"))?;

    if old_style.is_match(trimmed) || new_style.is_match(trimmed) {
        return Ok(trimmed.to_string());
    }

    sanitize_id(trimmed)
}

fn sanitize_id(raw: &str) -> Result<String> {
    let cleaned = raw.trim().trim_matches('/').trim_end_matches(".pdf").trim();
    if cleaned.is_empty() {
        bail!("无法从输入中解析 arXiv 论文 ID");
    }
    Ok(cleaned.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_request() -> SearchRequest {
        SearchRequest {
            raw_query: None,
            all_terms: vec![],
            title_terms: vec![],
            author_terms: vec![],
            category_terms: vec![],
            abstract_terms: vec![],
            id_list: vec![],
            start: 0,
            max_results: 10,
            sort_by: SortBy::Relevance,
            sort_order: SortOrder::Descending,
        }
    }

    #[test]
    fn builds_url_from_structured_terms() {
        let mut request = base_request();
        request.all_terms = vec!["transformer".to_string()];
        request.author_terms = vec!["Geoffrey Hinton".to_string()];
        request.category_terms = vec!["cs.LG".to_string()];

        let url = request
            .to_url_with_base(API_BASE_URL)
            .expect("to_url should succeed");

        assert!(url.contains(
            "search_query=all%3Atransformer%20AND%20au%3A%22Geoffrey%20Hinton%22%20AND%20cat%3Acs.LG"
        ));
        assert!(url.contains("start=0"));
        assert!(url.contains("max_results=10"));
        assert!(url.contains("sortBy=relevance"));
        assert!(url.contains("sortOrder=descending"));
    }

    #[test]
    fn builds_url_with_id_list_only() {
        let mut request = base_request();
        request.id_list = vec!["1706.03762".to_string(), "cs/0112017".to_string()];

        let url = request
            .to_url_with_base(API_BASE_URL)
            .expect("to_url should succeed");

        assert!(url.contains("id_list=1706.03762%2Ccs%2F0112017"));
        assert!(!url.contains("search_query="));
    }

    #[test]
    fn rejects_empty_query_and_id_list() {
        let request = base_request();
        let err = request
            .to_url_with_base(API_BASE_URL)
            .expect_err("expected query validation error");
        assert!(
            err.to_string().contains("至少提供一个查询条件"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn normalize_paper_id_accepts_common_inputs() {
        assert_eq!(
            normalize_paper_id("arXiv:1706.03762v7").expect("valid new style id"),
            "1706.03762v7"
        );
        assert_eq!(
            normalize_paper_id("https://arxiv.org/abs/cs/0112017").expect("valid abs url"),
            "cs/0112017"
        );
        assert_eq!(
            normalize_paper_id("https://arxiv.org/pdf/1706.03762.pdf").expect("valid pdf url"),
            "1706.03762"
        );
    }

    #[test]
    fn parse_feed_extracts_basic_fields() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/" xmlns:arxiv="http://arxiv.org/schemas/atom">
  <opensearch:totalResults>1</opensearch:totalResults>
  <opensearch:startIndex>0</opensearch:startIndex>
  <opensearch:itemsPerPage>1</opensearch:itemsPerPage>
  <entry>
    <id>http://arxiv.org/abs/1706.03762v7</id>
    <updated>2023-01-01T00:00:00Z</updated>
    <published>2017-06-12T17:57:37Z</published>
    <title> Attention Is All You Need </title>
    <summary> We propose a new simple network architecture. </summary>
    <author><name>Ashish Vaswani</name></author>
    <author><name>Noam Shazeer</name></author>
    <arxiv:doi>10.1000/test-doi</arxiv:doi>
    <link href="http://arxiv.org/abs/1706.03762v7" rel="alternate" type="text/html" />
    <link title="pdf" href="http://arxiv.org/pdf/1706.03762v7" rel="related" type="application/pdf" />
    <arxiv:primary_category term="cs.CL" scheme="http://arxiv.org/schemas/atom" />
    <category term="cs.CL" scheme="http://arxiv.org/schemas/atom" />
    <category term="cs.LG" scheme="http://arxiv.org/schemas/atom" />
  </entry>
</feed>"#;

        let response = parse_feed(xml).expect("feed should parse");
        assert_eq!(response.total_results, 1);
        assert_eq!(response.start_index, 0);
        assert_eq!(response.items_per_page, 1);
        assert_eq!(response.entries.len(), 1);

        let paper = &response.entries[0];
        assert_eq!(paper.short_id, "1706.03762v7");
        assert_eq!(paper.title, "Attention Is All You Need");
        assert_eq!(
            paper.summary,
            "We propose a new simple network architecture."
        );
        assert_eq!(paper.primary_category.as_deref(), Some("cs.CL"));
        assert_eq!(paper.authors.len(), 2);
        assert!(
            paper
                .pdf_url
                .as_deref()
                .is_some_and(|url| url.contains("/pdf/1706.03762v7"))
        );
    }

    #[test]
    fn to_url_with_custom_base_works() {
        let mut request = base_request();
        request.title_terms = vec!["attention".to_string()];
        let url = request
            .to_url_with_base("http://127.0.0.1:8000/api/query")
            .expect("to_url_with_base should succeed");

        assert!(url.starts_with("http://127.0.0.1:8000/api/query?"));
        assert!(url.contains("search_query=ti%3Aattention"));
    }

    #[tokio::test]
    async fn search_all_validates_zero_batch_size() {
        let client = ArxivClient::new().expect("client should build");
        let mut request = base_request();
        request.id_list = vec!["1706.03762".to_string()];

        let err = client
            .search_all(&request, Some(1), 0)
            .await
            .expect_err("zero batch size should fail");
        assert!(err.to_string().contains("batch_size 必须大于 0"));
    }
}
