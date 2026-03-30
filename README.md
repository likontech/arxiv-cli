# arxiv-cli

A Rust workspace for arXiv workflows:

- Search API: search, show, download, save
- OAI-PMH: identify, list-records, list-identifiers, get-record
- Local SQLite store and incremental sync checkpoint

<p align="left">
  <strong>Languages:</strong>
  English (current) · <a href="./README.zh-CN.md">简体中文</a>
</p>

## Features

- Unified CLI groups: `arxiv`, `oai`, `store`, `sync`
- Robust Search API client with retry/backoff and polite rate limiting
- OAI-PMH harvesting with `resumptionToken` pagination
- Local metadata indexing in SQLite (`papers`, `tags`, `sync_state`)
- Incremental sync state management for resumable harvesting

## Workspace Layout

```text
crates/
  arxiv-api/    # Search API client
  oai-pmh/      # OAI-PMH client
  paper-store/  # Local SQLite storage
  harvester/    # Incremental sync wrapper
  arxiv-cli/    # CLI entrypoint
```

## Quick Start

```bash
cargo build
cargo test --workspace
cargo run -p arxiv-cli -- --help
```

## CLI Overview

Top-level command groups:

- `arxiv`: Search API workflows
- `oai`: OAI-PMH workflows
- `store`: local paper management
- `sync`: incremental sync

---

## `arxiv` Commands

### Search

```bash
cargo run -p arxiv-cli -- arxiv search --query transformer --max-results 5
```

Useful flags:

- `--query`, `--title`, `--author`, `--category`, `--abstract-text`
- `--raw-query`
- `--id`
- `--start`, `--max-results`
- `--all-results`, `--batch-size`, `--limit`
- `--sort-by` (`relevance|last-updated-date|submitted-date`)
- `--sort-order` (`ascending|descending`)
- `--format` (`text|json`)

### Show / Download / Save

```bash
cargo run -p arxiv-cli -- arxiv show 1706.03762
cargo run -p arxiv-cli -- arxiv download 1706.03762 --format pdf
cargo run -p arxiv-cli -- arxiv download 1706.03762 --format source
cargo run -p arxiv-cli -- arxiv save 1706.03762 --db-path ./data/papers.db
```

---

## `oai` Commands

```bash
cargo run -p arxiv-cli -- oai identify --format json
cargo run -p arxiv-cli -- oai list-identifiers --metadata-prefix oai_dc --all --limit 100 --format json
cargo run -p arxiv-cli -- oai get-record --identifier oai:arXiv.org:1234.5678 --metadata-prefix oai_dc --format json
cargo run -p arxiv-cli -- oai list-records --metadata-prefix oai_dc --all --limit 50 --format json
```

Common flags:

- `--metadata-prefix` (default `oai_dc`)
- `--from`, `--until`, `--set`
- `--resumption-token`
- `--all`, `--limit`
- `--base-url`
- `--format`

---

## `store` Commands

```bash
cargo run -p arxiv-cli -- store list --limit 20 --format text
cargo run -p arxiv-cli -- store list --author bengio --format json
cargo run -p arxiv-cli -- store tag-add http://arxiv.org/abs/1706.03762v1 favorite
cargo run -p arxiv-cli -- store tag-remove http://arxiv.org/abs/1706.03762v1 favorite
```

---

## `sync` Commands

```bash
cargo run -p arxiv-cli -- sync oai --metadata-prefix oai_dc --state-key arxiv-oai
cargo run -p arxiv-cli -- sync oai --metadata-prefix oai_dc --from 2024-01-01 --until 2024-01-31 --state-key jan-2024
```

Current sync behavior:

- Maintains checkpoint state (`resumptionToken`, last record id)
- Focuses on state progression; does not persist full OAI records into `papers` yet

---

## Rate Limit Policy

Search API client follows a conservative policy:

- Respects `Crawl-delay: 15` from `arxiv.org/robots.txt`
- Retries with backoff on `429`
- Parses `Retry-After`

Recommended environment variable:

```bash
export ARXIV_CONTACT_EMAIL="you@example.com"
```

Optional endpoint overrides:

- `ARXIV_API_BASE_URL`
- `ARXIV_PDF_BASE_URL`
- `ARXIV_SOURCE_BASE_URL`

---

## Chinese Snapshot

For full Chinese documentation, see [`README.zh-CN.md`](./README.zh-CN.md).

- 核心命令组：`arxiv` / `oai` / `store` / `sync`
- 支持检索、下载、落库、标签管理、OAI 增量同步
- 已实现 `robots.txt` 友好限流与重试退避
