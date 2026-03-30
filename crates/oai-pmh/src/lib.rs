use anyhow::{Context, Result, bail};
use reqwest::{Client, Response, StatusCode};
use roxmltree::{Document, Node};
use serde::Serialize;
use tokio::time::{Duration, sleep};

const DEFAULT_BASE_URL: &str = "https://export.arxiv.org/oai2";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const MAX_RETRIES: usize = 3;
const RETRY_DELAY_MS: u64 = 1200;

#[derive(Debug, Clone)]
pub struct OaiClient {
    http: Client,
    base_url: String,
}

#[derive(Debug, Clone, Default)]
pub struct ListRecordsRequest {
    pub metadata_prefix: String,
    pub from: Option<String>,
    pub until: Option<String>,
    pub set: Option<String>,
    pub resumption_token: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ListIdentifiersRequest {
    pub metadata_prefix: String,
    pub from: Option<String>,
    pub until: Option<String>,
    pub set: Option<String>,
    pub resumption_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GetRecordRequest {
    pub identifier: String,
    pub metadata_prefix: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OaiIdentify {
    pub repository_name: Option<String>,
    pub base_url: Option<String>,
    pub protocol_version: Option<String>,
    pub admin_emails: Vec<String>,
    pub earliest_datestamp: Option<String>,
    pub deleted_record: Option<String>,
    pub granularity: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OaiHeader {
    pub identifier: Option<String>,
    pub datestamp: Option<String>,
    pub sets: Vec<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OaiRecord {
    pub identifier: Option<String>,
    pub datestamp: Option<String>,
    pub sets: Vec<String>,
    pub metadata_xml: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OaiListRecordsResponse {
    pub records: Vec<OaiRecord>,
    pub resumption_token: Option<String>,
    pub complete_list_size: Option<usize>,
    pub cursor: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OaiListIdentifiersResponse {
    pub headers: Vec<OaiHeader>,
    pub resumption_token: Option<String>,
    pub complete_list_size: Option<usize>,
    pub cursor: Option<usize>,
}

impl OaiClient {
    pub fn new(base_url: Option<String>) -> Result<Self> {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .context("创建 OAI HTTP 客户端失败")?;
        Ok(Self {
            http,
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        })
    }

    pub async fn identify(&self) -> Result<OaiIdentify> {
        let url = format!("{}?verb=Identify", self.base_url);
        let xml = self.fetch_text(&url).await?;
        parse_identify(&xml)
    }

    pub async fn get_record(&self, request: &GetRecordRequest) -> Result<OaiRecord> {
        if request.identifier.trim().is_empty() {
            bail!("identifier 不能为空");
        }
        if request.metadata_prefix.trim().is_empty() {
            bail!("metadata_prefix 不能为空");
        }

        let url = format!(
            "{}?verb=GetRecord&identifier={}&metadataPrefix={}",
            self.base_url,
            urlencoding::encode(request.identifier.trim()),
            urlencoding::encode(request.metadata_prefix.trim())
        );
        let xml = self.fetch_text(&url).await?;
        parse_get_record(&xml)
    }

    pub async fn list_records(
        &self,
        request: &ListRecordsRequest,
    ) -> Result<OaiListRecordsResponse> {
        let mut params = vec!["verb=ListRecords".to_string()];
        append_harvest_params(
            &mut params,
            &request.metadata_prefix,
            &request.from,
            &request.until,
            &request.set,
            &request.resumption_token,
        )?;

        let url = format!("{}?{}", self.base_url, params.join("&"));
        let xml = self.fetch_text(&url).await?;
        parse_list_records(&xml)
    }

    pub async fn list_records_all(
        &self,
        mut request: ListRecordsRequest,
        limit: Option<usize>,
    ) -> Result<Vec<OaiRecord>> {
        if request.resumption_token.is_none() && request.metadata_prefix.trim().is_empty() {
            bail!("metadata_prefix 不能为空");
        }

        let mut all = Vec::new();
        let mut remaining = limit.unwrap_or(usize::MAX);

        loop {
            let page = self.list_records(&request).await?;
            if page.records.is_empty() {
                break;
            }

            for record in page.records {
                if remaining == 0 {
                    return Ok(all);
                }
                all.push(record);
                remaining = remaining.saturating_sub(1);
            }

            let Some(token) = page.resumption_token else {
                break;
            };
            if token.trim().is_empty() {
                break;
            }

            request = ListRecordsRequest {
                metadata_prefix: request.metadata_prefix.clone(),
                from: None,
                until: None,
                set: None,
                resumption_token: Some(token),
            };

            if remaining == 0 {
                break;
            }
        }

        Ok(all)
    }

    pub async fn list_identifiers(
        &self,
        request: &ListIdentifiersRequest,
    ) -> Result<OaiListIdentifiersResponse> {
        let mut params = vec!["verb=ListIdentifiers".to_string()];
        append_harvest_params(
            &mut params,
            &request.metadata_prefix,
            &request.from,
            &request.until,
            &request.set,
            &request.resumption_token,
        )?;

        let url = format!("{}?{}", self.base_url, params.join("&"));
        let xml = self.fetch_text(&url).await?;
        parse_list_identifiers(&xml)
    }

    pub async fn list_identifiers_all(
        &self,
        mut request: ListIdentifiersRequest,
        limit: Option<usize>,
    ) -> Result<Vec<OaiHeader>> {
        if request.resumption_token.is_none() && request.metadata_prefix.trim().is_empty() {
            bail!("metadata_prefix 不能为空");
        }

        let mut all = Vec::new();
        let mut remaining = limit.unwrap_or(usize::MAX);

        loop {
            let page = self.list_identifiers(&request).await?;
            if page.headers.is_empty() {
                break;
            }

            for header in page.headers {
                if remaining == 0 {
                    return Ok(all);
                }
                all.push(header);
                remaining = remaining.saturating_sub(1);
            }

            let Some(token) = page.resumption_token else {
                break;
            };
            if token.trim().is_empty() {
                break;
            }

            request = ListIdentifiersRequest {
                metadata_prefix: request.metadata_prefix.clone(),
                from: None,
                until: None,
                set: None,
                resumption_token: Some(token),
            };

            if remaining == 0 {
                break;
            }
        }

        Ok(all)
    }

    async fn fetch_text(&self, url: &str) -> Result<String> {
        let response = self.send_with_retries(url).await?;
        response.text().await.context("读取 OAI-PMH 响应失败")
    }

    async fn send_with_retries(&self, url: &str) -> Result<Response> {
        for attempt in 0..=MAX_RETRIES {
            let response = self
                .http
                .get(url)
                .send()
                .await
                .with_context(|| format!("请求 OAI-PMH 失败: {url}"))?;

            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }

            if attempt < MAX_RETRIES
                && (status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
            {
                sleep(Duration::from_millis(RETRY_DELAY_MS * (attempt as u64 + 1))).await;
                continue;
            }

            let body = response.text().await.unwrap_or_default();
            bail!(
                "OAI-PMH 请求失败: {url} (status {}) {}",
                status.as_u16(),
                body
            );
        }

        bail!("OAI-PMH 请求失败: {url}")
    }
}

fn append_harvest_params(
    params: &mut Vec<String>,
    metadata_prefix: &str,
    from: &Option<String>,
    until: &Option<String>,
    set: &Option<String>,
    resumption_token: &Option<String>,
) -> Result<()> {
    if let Some(token) = resumption_token.as_ref() {
        if token.trim().is_empty() {
            bail!("resumption_token 不能为空");
        }
        params.push(format!("resumptionToken={}", urlencoding::encode(token)));
        return Ok(());
    }

    if metadata_prefix.trim().is_empty() {
        bail!("metadata_prefix 不能为空");
    }

    params.push(format!(
        "metadataPrefix={}",
        urlencoding::encode(metadata_prefix.trim())
    ));

    if let Some(value) = from.as_ref() {
        params.push(format!("from={}", urlencoding::encode(value.trim())));
    }
    if let Some(value) = until.as_ref() {
        params.push(format!("until={}", urlencoding::encode(value.trim())));
    }
    if let Some(value) = set.as_ref() {
        params.push(format!("set={}", urlencoding::encode(value.trim())));
    }

    Ok(())
}

fn parse_identify(xml: &str) -> Result<OaiIdentify> {
    let document = Document::parse(xml).context("解析 OAI XML 失败")?;
    let identify = find_descendant(document.root_element(), "Identify")
        .ok_or_else(|| anyhow::anyhow!("缺少 Identify 节点"))?;

    let repository_name = child_text(identify, "repositoryName");
    let base_url = child_text(identify, "baseURL");
    let protocol_version = child_text(identify, "protocolVersion");
    let admin_emails = identify
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "adminEmail")
        .filter_map(|n| n.text())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    Ok(OaiIdentify {
        repository_name,
        base_url,
        protocol_version,
        admin_emails,
        earliest_datestamp: child_text(identify, "earliestDatestamp"),
        deleted_record: child_text(identify, "deletedRecord"),
        granularity: child_text(identify, "granularity"),
    })
}

fn parse_get_record(xml: &str) -> Result<OaiRecord> {
    let document = Document::parse(xml).context("解析 OAI XML 失败")?;
    let get_record = find_descendant(document.root_element(), "GetRecord")
        .ok_or_else(|| anyhow::anyhow!("缺少 GetRecord 节点"))?;

    let record = get_record
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "record")
        .ok_or_else(|| anyhow::anyhow!("GetRecord 缺少 record 节点"))?;

    parse_record(record)
}

fn parse_list_records(xml: &str) -> Result<OaiListRecordsResponse> {
    let document = Document::parse(xml).context("解析 OAI XML 失败")?;
    let list_records = find_descendant(document.root_element(), "ListRecords")
        .ok_or_else(|| anyhow::anyhow!("缺少 ListRecords 节点"))?;

    let records = list_records
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "record")
        .map(parse_record)
        .collect::<Result<Vec<_>>>()?;

    let (resumption_token, complete_list_size, cursor) = parse_resumption_token(list_records);

    Ok(OaiListRecordsResponse {
        records,
        resumption_token,
        complete_list_size,
        cursor,
    })
}

fn parse_list_identifiers(xml: &str) -> Result<OaiListIdentifiersResponse> {
    let document = Document::parse(xml).context("解析 OAI XML 失败")?;
    let list_identifiers = find_descendant(document.root_element(), "ListIdentifiers")
        .ok_or_else(|| anyhow::anyhow!("缺少 ListIdentifiers 节点"))?;

    let headers = list_identifiers
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "header")
        .map(parse_header)
        .collect::<Result<Vec<_>>>()?;

    let (resumption_token, complete_list_size, cursor) = parse_resumption_token(list_identifiers);

    Ok(OaiListIdentifiersResponse {
        headers,
        resumption_token,
        complete_list_size,
        cursor,
    })
}

fn parse_resumption_token(node: Node<'_, '_>) -> (Option<String>, Option<usize>, Option<usize>) {
    let token_node = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "resumptionToken");

    let token = token_node
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let complete_list_size = token_node
        .and_then(|n| n.attribute("completeListSize"))
        .and_then(|v| v.parse::<usize>().ok());

    let cursor = token_node
        .and_then(|n| n.attribute("cursor"))
        .and_then(|v| v.parse::<usize>().ok());

    (token, complete_list_size, cursor)
}

fn parse_record(record: Node<'_, '_>) -> Result<OaiRecord> {
    let header = record
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "header")
        .ok_or_else(|| anyhow::anyhow!("record 缺少 header"))?;

    let parsed_header = parse_header(header)?;

    let metadata_xml = record
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "metadata")
        .map(|metadata| {
            metadata
                .children()
                .filter(|n| n.is_element())
                .map(|n| xml_fragment(n, xml_fragment::Mode::Normal))
                .collect::<String>()
        });

    Ok(OaiRecord {
        identifier: parsed_header.identifier,
        datestamp: parsed_header.datestamp,
        sets: parsed_header.sets,
        metadata_xml,
    })
}

fn parse_header(header: Node<'_, '_>) -> Result<OaiHeader> {
    if !(header.is_element() && header.tag_name().name() == "header") {
        bail!("节点不是 header");
    }

    let identifier = child_text(header, "identifier");
    let datestamp = child_text(header, "datestamp");
    let sets = header
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "setSpec")
        .filter_map(|n| n.text())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    let status = header.attribute("status").map(ToOwned::to_owned);

    Ok(OaiHeader {
        identifier,
        datestamp,
        sets,
        status,
    })
}

fn child_text(node: Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn find_descendant<'a, 'input>(node: Node<'a, 'input>, target: &str) -> Option<Node<'a, 'input>> {
    if node.is_element() && node.tag_name().name() == target {
        return Some(node);
    }
    for child in node.children() {
        if let Some(found) = find_descendant(child, target) {
            return Some(found);
        }
    }
    None
}

mod xml_fragment {
    #[derive(Clone, Copy)]
    pub enum Mode {
        Normal,
    }
}

fn xml_fragment(node: Node<'_, '_>, _mode: xml_fragment::Mode) -> String {
    let mut out = String::new();
    out.push('<');
    out.push_str(node.tag_name().name());
    for attr in node.attributes() {
        out.push(' ');
        out.push_str(attr.name());
        out.push_str("=\"");
        out.push_str(attr.value());
        out.push('"');
    }
    out.push('>');
    if let Some(text) = node.text() {
        out.push_str(text);
    }
    for child in node.children().filter(|n| n.is_element()) {
        out.push_str(&xml_fragment(child, xml_fragment::Mode::Normal));
    }
    out.push_str("</");
    out.push_str(node.tag_name().name());
    out.push('>');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_identify_works() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/">
  <responseDate>2026-03-30T00:00:00Z</responseDate>
  <request verb="Identify">https://export.arxiv.org/oai2</request>
  <Identify>
    <repositoryName>arXiv</repositoryName>
    <baseURL>https://export.arxiv.org/oai2</baseURL>
    <protocolVersion>2.0</protocolVersion>
    <adminEmail>help@arxiv.org</adminEmail>
    <earliestDatestamp>1991-08-14</earliestDatestamp>
    <deletedRecord>no</deletedRecord>
    <granularity>YYYY-MM-DD</granularity>
  </Identify>
</OAI-PMH>"#;

        let result = parse_identify(xml).expect("identify should parse");
        assert_eq!(result.repository_name.as_deref(), Some("arXiv"));
        assert_eq!(result.protocol_version.as_deref(), Some("2.0"));
        assert_eq!(result.admin_emails, vec!["help@arxiv.org"]);
    }

    #[test]
    fn parse_list_records_works() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/" xmlns:oai_dc="http://www.openarchives.org/OAI/2.0/oai_dc/">
  <ListRecords>
    <record>
      <header>
        <identifier>oai:arXiv.org:1234.5678</identifier>
        <datestamp>2026-03-30</datestamp>
        <setSpec>cs</setSpec>
      </header>
      <metadata>
        <oai_dc:dc>
          <title>Test Paper</title>
        </oai_dc:dc>
      </metadata>
    </record>
    <resumptionToken completeListSize="100" cursor="0">token-1</resumptionToken>
  </ListRecords>
</OAI-PMH>"#;

        let result = parse_list_records(xml).expect("list records should parse");
        assert_eq!(result.records.len(), 1);
        assert_eq!(
            result.records[0].identifier.as_deref(),
            Some("oai:arXiv.org:1234.5678")
        );
        assert_eq!(result.resumption_token.as_deref(), Some("token-1"));
        assert_eq!(result.complete_list_size, Some(100));
    }

    #[test]
    fn parse_list_identifiers_works() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/">
  <ListIdentifiers>
    <header>
      <identifier>oai:arXiv.org:1111.00001</identifier>
      <datestamp>2026-03-30</datestamp>
      <setSpec>cs</setSpec>
    </header>
    <resumptionToken completeListSize="10" cursor="0">next-token</resumptionToken>
  </ListIdentifiers>
</OAI-PMH>"#;

        let result = parse_list_identifiers(xml).expect("list identifiers should parse");
        assert_eq!(result.headers.len(), 1);
        assert_eq!(
            result.headers[0].identifier.as_deref(),
            Some("oai:arXiv.org:1111.00001")
        );
        assert_eq!(result.resumption_token.as_deref(), Some("next-token"));
    }

    #[test]
    fn parse_get_record_works() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/">
  <GetRecord>
    <record>
      <header>
        <identifier>oai:arXiv.org:1234.5678</identifier>
        <datestamp>2026-03-30</datestamp>
      </header>
      <metadata>
        <dc>
          <title>Record Title</title>
        </dc>
      </metadata>
    </record>
  </GetRecord>
</OAI-PMH>"#;

        let record = parse_get_record(xml).expect("get record should parse");
        assert_eq!(
            record.identifier.as_deref(),
            Some("oai:arXiv.org:1234.5678")
        );
        assert!(
            record
                .metadata_xml
                .as_deref()
                .is_some_and(|value| value.contains("Record Title"))
        );
    }
}
