use std::path::PathBuf;

use anyhow::{Context, Result};
use arxiv_api::Paper;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPaper {
 pub id: String,
 pub short_id: String,
 pub title: String,
 pub summary: String,
 pub authors: Vec<String>,
 pub categories: Vec<String>,
 pub published: Option<String>,
 pub updated: Option<String>,
 pub pdf_url: Option<String>,
 pub local_path: Option<String>,
 pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct QueryFilter {
 pub author_keyword: Option<String>,
 pub category: Option<String>,
 pub tag: Option<String>,
 pub title_keyword: Option<String>,
 pub limit: usize,
}

impl Default for QueryFilter {
 fn default() -> Self {
 Self {
 author_keyword: None,
 category: None,
 tag: None,
 title_keyword: None,
 limit: 20,
 }
 }
}

#[derive(Debug, Clone)]
pub struct PaperStore {
 db_path: PathBuf,
}

impl PaperStore {
 pub fn new(db_path: Option<PathBuf>) -> Result<Self> {
 let path = match db_path {
 Some(value) => value,
 None => default_db_path()?,
 };

 if let Some(parent) = path.parent() {
 std::fs::create_dir_all(parent)
 .with_context(|| format!("创建目录失败: {}", parent.display()))?;
 }

 let store = Self { db_path: path };
 store.init()?;
 Ok(store)
 }

 pub fn init(&self) -> Result<()> {
 let conn = self.conn()?;
 conn.execute_batch(
 "
 CREATE TABLE IF NOT EXISTS papers (
 id TEXT PRIMARY KEY,
 short_id TEXT NOT NULL,
 title TEXT NOT NULL,
 summary TEXT NOT NULL,
 authors_json TEXT NOT NULL,
 categories_json TEXT NOT NULL,
 published TEXT,
 updated TEXT,
 pdf_url TEXT,
 local_path TEXT
 );

 CREATE TABLE IF NOT EXISTS tags (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 paper_id TEXT NOT NULL,
 tag TEXT NOT NULL,
 UNIQUE(paper_id, tag),
 FOREIGN KEY(paper_id) REFERENCES papers(id) ON DELETE CASCADE
 );

 CREATE TABLE IF NOT EXISTS sync_state (
 key TEXT PRIMARY KEY,
 value TEXT NOT NULL
 );
 ",
 )?;
 Ok(())
 }

 pub fn save_paper(&self, paper: &Paper, local_path: Option<&str>) -> Result<()> {
 let conn = self.conn()?;
 let authors_json = serde_json::to_string(&paper.authors)?;
 let categories_json = serde_json::to_string(&paper.categories)?;

 conn.execute(
 "
 INSERT INTO papers (
 id, short_id, title, summary,
 authors_json, categories_json,
 published, updated, pdf_url, local_path
 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
 ON CONFLICT(id) DO UPDATE SET
 short_id=excluded.short_id,
 title=excluded.title,
 summary=excluded.summary,
 authors_json=excluded.authors_json,
 categories_json=excluded.categories_json,
 published=excluded.published,
 updated=excluded.updated,
 pdf_url=excluded.pdf_url,
 local_path=COALESCE(excluded.local_path, papers.local_path)
 ",
 params![
 paper.id,
 paper.short_id,
 paper.title,
 paper.summary,
 authors_json,
 categories_json,
 paper.published,
 paper.updated,
 paper.pdf_url,
 local_path
 ],
 )?;

 Ok(())
 }

 pub fn add_tag(&self, paper_id: &str, tag: &str) -> Result<()> {
 let conn = self.conn()?;
 conn.execute(
 "INSERT OR IGNORE INTO tags (paper_id, tag) VALUES (?1, ?2)",
 params![paper_id, tag],
 )?;
 Ok(())
 }

 pub fn remove_tag(&self, paper_id: &str, tag: &str) -> Result<()> {
 let conn = self.conn()?;
 conn.execute(
 "DELETE FROM tags WHERE paper_id = ?1 AND tag = ?2",
 params![paper_id, tag],
 )?;
 Ok(())
 }

 pub fn list_papers(&self, filter: &QueryFilter) -> Result<Vec<StoredPaper>> {
 let conn = self.conn()?;
 let mut stmt = conn.prepare(
 "
 SELECT id, short_id, title, summary, authors_json, categories_json,
 published, updated, pdf_url, local_path
 FROM papers
 ORDER BY COALESCE(updated, published, '') DESC
 LIMIT ?1
 ",
 )?;

 let rows = stmt.query_map(params![filter.limit as i64], |row| {
 let id: String = row.get(0)?;
 let short_id: String = row.get(1)?;
 let title: String = row.get(2)?;
 let summary: String = row.get(3)?;
 let authors_json: String = row.get(4)?;
 let categories_json: String = row.get(5)?;
 let published: Option<String> = row.get(6)?;
 let updated: Option<String> = row.get(7)?;
 let pdf_url: Option<String> = row.get(8)?;
 let local_path: Option<String> = row.get(9)?;

 let authors = serde_json::from_str::<Vec<String>>(&authors_json).unwrap_or_default();
 let categories =
 serde_json::from_str::<Vec<String>>(&categories_json).unwrap_or_default();

 Ok(StoredPaper {
 id,
 short_id,
 title,
 summary,
 authors,
 categories,
 published,
 updated,
 pdf_url,
 local_path,
 tags: vec![],
 })
 })?;

 let mut papers = Vec::new();
 for row in rows {
 let mut paper = row?;
 if let Some(keyword) = filter.title_keyword.as_ref()
 && !paper.title.to_lowercase().contains(&keyword.to_lowercase())
 {
 continue;
 }
 if let Some(author_keyword) = filter.author_keyword.as_ref()
 && !paper
 .authors
 .iter()
 .any(|a| a.to_lowercase().contains(&author_keyword.to_lowercase()))
 {
 continue;
 }
 if let Some(category) = filter.category.as_ref()
 && !paper.categories.iter().any(|c| c == category)
 {
 continue;
 }

 paper.tags = self.paper_tags(&paper.id)?;

 if let Some(tag) = filter.tag.as_ref() && !paper.tags.iter().any(|t| t == tag) {
 continue;
 }

 papers.push(paper);
 }

 Ok(papers)
 }

 pub fn set_sync_state(&self, key: &str, value: &str) -> Result<()> {
 let conn = self.conn()?;
 conn.execute(
 "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
 params![key, value],
 )?;
 Ok(())
 }

 pub fn get_sync_state(&self, key: &str) -> Result<Option<String>> {
 let conn = self.conn()?;
 let value = conn
 .query_row(
 "SELECT value FROM sync_state WHERE key = ?1",
 params![key],
 |row| row.get(0),
 )
 .optional()?;
 Ok(value)
 }

 pub fn db_path(&self) -> &PathBuf {
 &self.db_path
 }

 fn conn(&self) -> Result<Connection> {
 let conn = Connection::open(&self.db_path)
 .with_context(|| format!("打开数据库失败: {}", self.db_path.display()))?;
 Ok(conn)
 }

 fn paper_tags(&self, paper_id: &str) -> Result<Vec<String>> {
 let conn = self.conn()?;
 let mut stmt = conn.prepare("SELECT tag FROM tags WHERE paper_id = ?1 ORDER BY tag")?;
 let rows = stmt.query_map(params![paper_id], |row| row.get::<_, String>(0))?;
 let mut tags = Vec::new();
 for row in rows {
 tags.push(row?);
 }
 Ok(tags)
 }
}

fn default_db_path() -> Result<PathBuf> {
 let base = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法获取用户 home 目录"))?;
 Ok(base.join(".arxiv-cli").join("papers.db"))
}

#[cfg(test)]
mod tests {
 use super::*;
 use tempfile::tempdir;

 fn test_paper() -> Paper {
 Paper {
 id: "http://arxiv.org/abs/2301.00001v1".to_string(),
 short_id: "2301.00001".to_string(),
 title: "Test Paper".to_string(),
 summary: "Abstract".to_string(),
 published: Some("2023-01-01".to_string()),
 updated: Some("2023-01-02".to_string()),
 authors: vec!["Alice".to_string(), "Bob".to_string()],
 categories: vec!["cs.AI".to_string()],
 primary_category: Some("cs.AI".to_string()),
 pdf_url: Some("https://arxiv.org/pdf/2301.00001".to_string()),
 doi: None,
 comment: None,
 journal_ref: None,
 }
 }

 #[test]
 fn test_save_and_list() {
 let dir = tempdir().unwrap();
 let db = dir.path().join("test.db");
 let store = PaperStore::new(Some(db)).unwrap();

 let paper = test_paper();
 store.save_paper(&paper, None).unwrap();

 let filter = QueryFilter::default();
 let list = store.list_papers(&filter).unwrap();
 assert_eq!(list.len(), 1);
 assert_eq!(list[0].id, paper.id);
 }

 #[test]
 fn test_tags() {
 let dir = tempdir().unwrap();
 let db = dir.path().join("test.db");
 let store = PaperStore::new(Some(db)).unwrap();

 let paper = test_paper();
 store.save_paper(&paper, None).unwrap();

 store.add_tag(&paper.id, "favorite").unwrap();
 store.add_tag(&paper.id, "work").unwrap();

 let filter = QueryFilter { tag: Some("favorite".to_string()), ..Default::default() };
 let list = store.list_papers(&filter).unwrap();
 assert_eq!(list.len(), 1);
 assert!(list[0].tags.contains(&"favorite".to_string()));

 store.remove_tag(&paper.id, "favorite").unwrap();
 let filter = QueryFilter { tag: Some("favorite".to_string()), ..Default::default() };
 let list = store.list_papers(&filter).unwrap();
 assert!(list.is_empty());
 }

 #[test]
 fn test_sync_state() {
 let dir = tempdir().unwrap();
 let db = dir.path().join("test.db");
 let store = PaperStore::new(Some(db)).unwrap();

 store.set_sync_state("job1", "token123").unwrap();
 let value = store.get_sync_state("job1").unwrap();
 assert_eq!(value, Some("token123".to_string()));

 store.set_sync_state("job1", "token456").unwrap();
 let value = store.get_sync_state("job1").unwrap();
 assert_eq!(value, Some("token456".to_string()));
 }
}
