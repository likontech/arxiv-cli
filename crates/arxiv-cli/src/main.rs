use std::path::PathBuf;

use anyhow::{Result, bail};
use arxiv_api::{
 ArxivClient, DownloadFormat, Paper, SearchRequest, SearchResponse, SortBy, SortOrder,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use harvester::{SyncConfig, sync_oai_records};
use oai_pmh::{
 GetRecordRequest, ListIdentifiersRequest, ListRecordsRequest, OaiClient, OaiHeader,
 OaiIdentify, OaiRecord,
};
use paper_store::{PaperStore, QueryFilter};

#[derive(Parser, Debug)]
#[command(author, version, about = "arXiv CLI with Search + OAI-PMH + Store + Sync", long_about = None)]
struct Cli {
 #[command(subcommand)]
 command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
 /// arXiv Search API utilities.
 Arxiv(ArxivGroup),
 /// OAI-PMH utilities.
 Oai(OaiGroup),
 /// Local paper store utilities.
 Store(StoreGroup),
 /// Incremental sync workflows.
 Sync(SyncGroup),
}

#[derive(Args, Debug)]
struct ArxivGroup {
 #[command(subcommand)]
 command: ArxivCommands,
}

#[derive(Args, Debug)]
struct OaiGroup {
 #[command(subcommand)]
 command: OaiCommands,
}

#[derive(Args, Debug)]
struct StoreGroup {
 #[command(subcommand)]
 command: StoreCommands,
}

#[derive(Args, Debug)]
struct SyncGroup {
 #[command(subcommand)]
 command: SyncCommands,
}

#[derive(Subcommand, Debug)]
enum ArxivCommands {
 /// Search papers with arXiv API syntax or structured filters.
 Search(SearchArgs),
 /// Fetch a single paper by arXiv ID.
 Show(ShowArgs),
 /// Download paper PDF or source archive.
 Download(DownloadArgs),
 /// Save paper metadata to local store.
 Save(SaveArgs),
}

#[derive(Subcommand, Debug)]
enum OaiCommands {
 /// Identify repository information.
 Identify(OaiIdentifyArgs),
 /// List records with optional pagination.
 ListRecords(OaiListRecordsArgs),
 /// List identifiers with optional pagination.
 ListIdentifiers(OaiListIdentifiersArgs),
 /// Get a single record by identifier.
 GetRecord(OaiGetRecordArgs),
}

#[derive(Subcommand, Debug)]
enum StoreCommands {
 /// List papers from local store.
 List(StoreListArgs),
 /// Add a tag to a paper.
 TagAdd(StoreTagArgs),
 /// Remove a tag from a paper.
 TagRemove(StoreTagArgs),
}

#[derive(Subcommand, Debug)]
enum SyncCommands {
 /// Sync records using OAI-PMH with checkpoint.
 Oai(SyncOaiArgs),
}

#[derive(Args, Debug)]
struct SearchArgs {
 #[arg(long)]
 raw_query: Option<String>,
 #[arg(long = "query")]
 query_terms: Vec<String>,
 #[arg(long = "title")]
 title_terms: Vec<String>,
 #[arg(long = "author")]
 author_terms: Vec<String>,
 #[arg(long = "category")]
 category_terms: Vec<String>,
 #[arg(long = "abstract-text")]
 abstract_terms: Vec<String>,
 #[arg(long = "id", value_delimiter = ',')]
 ids: Vec<String>,
 #[arg(long, default_value_t =0)]
 start: usize,
 #[arg(long = "max-results", default_value_t =10)]
 max_results: usize,
 #[arg(long = "all-results")]
 all_results: bool,
 #[arg(long)]
 limit: Option<usize>,
 #[arg(long = "batch-size", default_value_t =100)]
 batch_size: usize,
 #[arg(long, value_enum, default_value_t = SortByArg::Relevance)]
 sort_by: SortByArg,
 #[arg(long, value_enum, default_value_t = SortOrderArg::Descending)]
 sort_order: SortOrderArg,
 #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
 format: OutputFormat,
}

#[derive(Args, Debug)]
struct ShowArgs {
 id: String,
 #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
 format: OutputFormat,
}

#[derive(Args, Debug)]
struct DownloadArgs {
 id_or_url: String,
 #[arg(long, value_enum, default_value_t = DownloadKind::Pdf)]
 format: DownloadKind,
 #[arg(long)]
 output: Option<PathBuf>,
 #[arg(long = "force")]
 overwrite: bool,
}

#[derive(Args, Debug)]
struct SaveArgs {
 id: String,
 #[arg(long)]
 local_path: Option<String>,
 #[arg(long)]
 db_path: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct OaiIdentifyArgs {
 #[arg(long)]
 base_url: Option<String>,
 #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
 format: OutputFormat,
}

#[derive(Args, Debug)]
struct OaiListRecordsArgs {
 #[arg(long)]
 base_url: Option<String>,
 #[arg(long)]
 metadata_prefix: Option<String>,
 #[arg(long)]
 from: Option<String>,
 #[arg(long)]
 until: Option<String>,
 #[arg(long)]
 set: Option<String>,
 #[arg(long)]
 resumption_token: Option<String>,
 #[arg(long = "all")]
 all: bool,
 #[arg(long)]
 limit: Option<usize>,
 #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
 format: OutputFormat,
}

#[derive(Args, Debug)]
struct OaiListIdentifiersArgs {
 #[arg(long)]
 base_url: Option<String>,
 #[arg(long)]
 metadata_prefix: Option<String>,
 #[arg(long)]
 from: Option<String>,
 #[arg(long)]
 until: Option<String>,
 #[arg(long)]
 set: Option<String>,
 #[arg(long)]
 resumption_token: Option<String>,
 #[arg(long = "all")]
 all: bool,
 #[arg(long)]
 limit: Option<usize>,
 #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
 format: OutputFormat,
}

#[derive(Args, Debug)]
struct OaiGetRecordArgs {
 #[arg(long)]
 base_url: Option<String>,
 #[arg(long)]
 identifier: String,
 #[arg(long, default_value = "oai_dc")]
 metadata_prefix: String,
 #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
 format: OutputFormat,
}

#[derive(Args, Debug)]
struct StoreListArgs {
 #[arg(long)]
 db_path: Option<PathBuf>,
 #[arg(long)]
 author: Option<String>,
 #[arg(long)]
 category: Option<String>,
 #[arg(long)]
 tag: Option<String>,
 #[arg(long)]
 title: Option<String>,
 #[arg(long, default_value_t =20)]
 limit: usize,
 #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
 format: OutputFormat,
}

#[derive(Args, Debug)]
struct StoreTagArgs {
 id: String,
 tag: String,
 #[arg(long)]
 db_path: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct SyncOaiArgs {
 #[arg(long)]
 base_url: Option<String>,
 #[arg(long, default_value = "oai_dc")]
 metadata_prefix: String,
 #[arg(long)]
 from: Option<String>,
 #[arg(long)]
 until: Option<String>,
 #[arg(long)]
 set: Option<String>,
 #[arg(long)]
 limit: Option<usize>,
 #[arg(long, default_value = "arxiv-oai")]
 state_key: String,
 #[arg(long)]
 db_path: Option<PathBuf>,
 #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
 format: OutputFormat,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum OutputFormat {
 Text,
 Json,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum SortByArg {
 Relevance,
 LastUpdatedDate,
 SubmittedDate,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum SortOrderArg {
 Ascending,
 Descending,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum DownloadKind {
 Pdf,
 Source,
}

impl From<SortByArg> for SortBy {
 fn from(value: SortByArg) -> Self {
 match value {
 SortByArg::Relevance => Self::Relevance,
 SortByArg::LastUpdatedDate => Self::LastUpdatedDate,
 SortByArg::SubmittedDate => Self::SubmittedDate,
 }
 }
}

impl From<SortOrderArg> for SortOrder {
 fn from(value: SortOrderArg) -> Self {
 match value {
 SortOrderArg::Ascending => Self::Ascending,
 SortOrderArg::Descending => Self::Descending,
 }
 }
}

impl From<DownloadKind> for DownloadFormat {
 fn from(value: DownloadKind) -> Self {
 match value {
 DownloadKind::Pdf => Self::Pdf,
 DownloadKind::Source => Self::Source,
 }
 }
}

#[tokio::main]
async fn main() -> Result<()> {
 let cli = Cli::parse();

 match cli.command {
 Commands::Arxiv(group) => run_arxiv(group.command).await?,
 Commands::Oai(group) => run_oai(group.command).await?,
 Commands::Store(group) => run_store(group.command)?,
 Commands::Sync(group) => run_sync(group.command).await?,
 }

 Ok(())
}

async fn run_arxiv(command: ArxivCommands) -> Result<()> {
 let client = ArxivClient::new()?;

 match command {
 ArxivCommands::Search(args) => {
 let request = SearchRequest {
 raw_query: args.raw_query,
 all_terms: args.query_terms,
 title_terms: args.title_terms,
 author_terms: args.author_terms,
 category_terms: args.category_terms,
 abstract_terms: args.abstract_terms,
 id_list: args.ids,
 start: args.start,
 max_results: args.max_results,
 sort_by: args.sort_by.into(),
 sort_order: args.sort_order.into(),
 };
 let response = if args.all_results {
 client
 .search_all(&request, args.limit, args.batch_size)
 .await?
 } else {
 client.search(&request).await?
 };
 print_search_response(&response, args.format);
 }
 ArxivCommands::Show(args) => {
 let request = SearchRequest {
 raw_query: None,
 all_terms: Vec::new(),
 title_terms: Vec::new(),
 author_terms: Vec::new(),
 category_terms: Vec::new(),
 abstract_terms: Vec::new(),
 id_list: vec![args.id],
 start: 0,
 max_results: 1,
 sort_by: SortBy::Relevance,
 sort_order: SortOrder::Descending,
 };
 let response = client.search(&request).await?;
 if let Some(paper) = response.entries.first() {
 print_single_paper(paper, args.format);
 } else {
 bail!("未找到对应论文");
 }
 }
 ArxivCommands::Download(args) => {
 let output = client
 .download(
 &args.id_or_url,
 args.format.into(),
 args.output.as_deref(),
 args.overwrite,
 )
 .await?;
 println!("Saved to {}", output.display());
 }
 ArxivCommands::Save(args) => {
 let request = SearchRequest {
 raw_query: None,
 all_terms: Vec::new(),
 title_terms: Vec::new(),
 author_terms: Vec::new(),
 category_terms: Vec::new(),
 abstract_terms: Vec::new(),
 id_list: vec![args.id.clone()],
 start: 0,
 max_results: 1,
 sort_by: SortBy::Relevance,
 sort_order: SortOrder::Descending,
 };
 let response = client.search(&request).await?;
 let paper = response
 .entries
 .first()
 .ok_or_else(|| anyhow::anyhow!("未找到论文 {}", args.id))?;
 let store = PaperStore::new(args.db_path)?;
 store.save_paper(paper, args.local_path.as_deref())?;
 println!("已保存 {} -> {}", paper.id, store.db_path().display());
 }
 }

 Ok(())
}

async fn run_oai(command: OaiCommands) -> Result<()> {
 match command {
 OaiCommands::Identify(args) => {
 let client = OaiClient::new(args.base_url)?;
 let identify = client.identify().await?;
 print_identify(&identify, args.format);
 }
 OaiCommands::ListRecords(args) => {
 let client = OaiClient::new(args.base_url)?;
 let request = ListRecordsRequest {
 metadata_prefix: args.metadata_prefix.unwrap_or_else(|| "oai_dc".to_string()),
 from: args.from,
 until: args.until,
 set: args.set,
 resumption_token: args.resumption_token,
 };

 if args.all {
 let records = client.list_records_all(request, args.limit).await?;
 print_records(&records, args.format);
 } else {
 let page = client.list_records(&request).await?;
 if matches!(args.format, OutputFormat::Json) {
 println!("{}", serde_json::to_string_pretty(&page)?);
 } else {
 print_records(&page.records, args.format);
 if let Some(token) = page.resumption_token {
 println!("next resumptionToken: {token}");
 }
 }
 }
 }
 OaiCommands::ListIdentifiers(args) => {
 let client = OaiClient::new(args.base_url)?;
 let request = ListIdentifiersRequest {
 metadata_prefix: args.metadata_prefix.unwrap_or_else(|| "oai_dc".to_string()),
 from: args.from,
 until: args.until,
 set: args.set,
 resumption_token: args.resumption_token,
 };

 if args.all {
 let headers = client.list_identifiers_all(request, args.limit).await?;
 print_headers(&headers, args.format);
 } else {
 let page = client.list_identifiers(&request).await?;
 if matches!(args.format, OutputFormat::Json) {
 println!("{}", serde_json::to_string_pretty(&page)?);
 } else {
 print_headers(&page.headers, args.format);
 if let Some(token) = page.resumption_token {
 println!("next resumptionToken: {token}");
 }
 }
 }
 }
 OaiCommands::GetRecord(args) => {
 let client = OaiClient::new(args.base_url)?;
 let request = GetRecordRequest {
 identifier: args.identifier,
 metadata_prefix: args.metadata_prefix,
 };
 let record = client.get_record(&request).await?;
 print_record(&record, args.format);
 }
 }

 Ok(())
}

fn run_store(command: StoreCommands) -> Result<()> {
 match command {
 StoreCommands::List(args) => {
 let store = PaperStore::new(args.db_path)?;
 let filter = QueryFilter {
 author_keyword: args.author,
 category: args.category,
 tag: args.tag,
 title_keyword: args.title,
 limit: args.limit,
 };
 let papers = store.list_papers(&filter)?;
 match args.format {
 OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&papers)?),
 OutputFormat::Text => {
 for p in papers {
 println!("{} {} [{}]", p.short_id, p.title, p.categories.join(","));
 if !p.authors.is_empty() {
 println!(" Authors: {}", p.authors.join(", "));
 }
 if let Some(path) = p.local_path {
 println!(" Local: {}", path);
 }
 if !p.tags.is_empty() {
 println!(" Tags: {}", p.tags.join(", "));
 }
 println!();
 }
 }
 }
 }
 StoreCommands::TagAdd(args) => {
 let store = PaperStore::new(args.db_path)?;
 store.add_tag(&args.id, &args.tag)?;
 println!("已添加标签 '{}' 到 {}", args.tag, args.id);
 }
 StoreCommands::TagRemove(args) => {
 let store = PaperStore::new(args.db_path)?;
 store.remove_tag(&args.id, &args.tag)?;
 println!("已移除标签 '{}' 从 {}", args.tag, args.id);
 }
 }
 Ok(())
}

async fn run_sync(command: SyncCommands) -> Result<()> {
 match command {
 SyncCommands::Oai(args) => {
 let store = PaperStore::new(args.db_path)?;
 let config = SyncConfig {
 base_url: args.base_url,
 metadata_prefix: args.metadata_prefix,
 from: args.from,
 until: args.until,
 set: args.set,
 limit: args.limit,
 state_key: args.state_key,
 };
 let result = sync_oai_records(&store, config).await?;
 match args.format {
 OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
 OutputFormat::Text => {
 println!("Fetched {} records", result.fetched_records);
 if let Some(token) = result.last_token {
 println!("Next resumptionToken: {}", token);
 }
 }
 }
 }
 }
 Ok(())
}

fn print_identify(identify: &OaiIdentify, format: OutputFormat) {
 match format {
 OutputFormat::Json => println!(
 "{}",
 serde_json::to_string_pretty(identify).expect("serialize identify")
 ),
 OutputFormat::Text => {
 println!(
 "Repository: {}",
 identify.repository_name.as_deref().unwrap_or("-")
 );
 println!("Base URL: {}", identify.base_url.as_deref().unwrap_or("-"));
 println!(
 "Protocol: {}",
 identify.protocol_version.as_deref().unwrap_or("-")
 );
 if !identify.admin_emails.is_empty() {
 println!("Admins: {}", identify.admin_emails.join(", "));
 }
 println!(
 "Earliest Datestamp: {}",
 identify.earliest_datestamp.as_deref().unwrap_or("-")
 );
 }
 }
}

fn print_records(records: &[OaiRecord], format: OutputFormat) {
 match format {
 OutputFormat::Json => println!("{}", serde_json::to_string_pretty(records).expect("serialize records")),
 OutputFormat::Text => {
 for r in records {
 if let Some(id) = r.identifier.as_deref() {
 println!("ID: {}", id);
 }
 if let Some(date) = r.datestamp.as_deref() {
 println!("Date: {}", date);
 }
 println!("---");
 }
 }
 }
}

fn print_headers(headers: &[OaiHeader], format: OutputFormat) {
 match format {
 OutputFormat::Json => println!("{}", serde_json::to_string_pretty(headers).expect("serialize headers")),
 OutputFormat::Text => {
 for h in headers {
 if let Some(id) = h.identifier.as_deref() {
 println!("{}", id);
 }
 }
 }
 }
}

fn print_record(record: &OaiRecord, format: OutputFormat) {
 match format {
 OutputFormat::Json => println!("{}", serde_json::to_string_pretty(record).expect("serialize record")),
 OutputFormat::Text => {
 if let Some(id) = record.identifier.as_deref() {
 println!("ID: {}", id);
 }
 if let Some(date) = record.datestamp.as_deref() {
 println!("Date: {}", date);
 }
 }
 }
}

fn print_search_response(response: &SearchResponse, format: OutputFormat) {
 match format {
 OutputFormat::Json => {
 let json = serde_json::to_string_pretty(response).expect("serialize");
 println!("{}", json);
 }
 OutputFormat::Text => {
 for entry in &response.entries {
 println!(
 "{} | {} | {}",
 entry.short_id,
 entry.published.as_deref().unwrap_or("-"),
 entry.title
 );
 println!("Authors: {}", entry.authors.join(", "));
 println!("Categories: {}", entry.categories.join(", "));
 let s = entry.summary.replace('\n', " ");
 let s = if s.len() > 200 {
 format!("{}...", &s[..200])
 } else {
 s
 };
 println!("Summary: {}", s);
 println!();
 }
 }
 }
}

fn print_single_paper(paper: &Paper, format: OutputFormat) {
 match format {
 OutputFormat::Json => {
 let json = serde_json::to_string_pretty(paper).expect("serialize");
 println!("{}", json);
 }
 OutputFormat::Text => {
 println!("ID: {}", paper.id);
 println!("Title: {}", paper.title);
 println!("Authors: {}", paper.authors.join(", "));
 println!("Categories: {}", paper.categories.join(", "));
 println!("Published: {}", paper.published.as_deref().unwrap_or("-"));
 println!("Updated: {}", paper.updated.as_deref().unwrap_or("-"));
 println!("Summary: {}", paper.summary);
 if let Some(ref pdf) = paper.pdf_url {
 println!("PDF: {}", pdf);
 }
 }
 }
}
