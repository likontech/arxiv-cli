# arxiv-cli（简体中文）

<p align="left">
  <strong>语言：</strong>
  <a href="./README.md">English</a> · 简体中文（当前）
</p>

一个基于 Rust 的 arXiv 工具集工作区，统一了 Search API 与 OAI-PMH 两类流程，并提供本地 SQLite 索引与增量同步状态。

## 功能

- 统一命令组：`arxiv` / `oai` / `store` / `sync`
- Search API：检索、详情、下载、保存
- OAI-PMH：Identify / ListRecords / ListIdentifiers / GetRecord
- 本地论文库：标签管理、条件筛选
- 增量同步：基于 `resumptionToken` 的 checkpoint

## 工作区结构

```text
crates/
  arxiv-api/    # Search API 客户端
  oai-pmh/      # OAI-PMH 客户端
  paper-store/  # SQLite 存储
  harvester/    # 增量同步封装
  arxiv-cli/    # CLI 入口
```

## 构建与测试

```bash
cargo build
cargo test --workspace
cargo llvm-cov --workspace
```

## 使用总览

```bash
cargo run -p arxiv-cli -- --help
```

### `arxiv`（检索/下载/落库）

```bash
cargo run -p arxiv-cli -- arxiv search --query transformer --max-results 5
cargo run -p arxiv-cli -- arxiv show 1706.03762
cargo run -p arxiv-cli -- arxiv download 1706.03762 --format pdf
cargo run -p arxiv-cli -- arxiv save 1706.03762 --db-path ./data/papers.db
```

检索常用参数：

- `--query`、`--title`、`--author`、`--category`、`--abstract-text`
- `--raw-query`
- `--id`
- `--start` / `--max-results`
- `--all-results` / `--batch-size` / `--limit`
- `--sort-by` / `--sort-order`
- `--format`

### `oai`（元数据收割）

```bash
cargo run -p arxiv-cli -- oai identify --format json
cargo run -p arxiv-cli -- oai list-identifiers --metadata-prefix oai_dc --all --limit 100 --format json
cargo run -p arxiv-cli -- oai get-record --identifier oai:arXiv.org:1234.5678 --metadata-prefix oai_dc --format json
cargo run -p arxiv-cli -- oai list-records --metadata-prefix oai_dc --all --limit 50 --format json
```

常用参数：

- `--metadata-prefix`（默认 `oai_dc`）
- `--from` / `--until` / `--set`
- `--resumption-token`
- `--all` / `--limit`
- `--base-url`
- `--format`

### `store`（本地论文库）

```bash
cargo run -p arxiv-cli -- store list --limit 20 --format text
cargo run -p arxiv-cli -- store list --author bengio --format json
cargo run -p arxiv-cli -- store tag-add http://arxiv.org/abs/1706.03762v1 favorite
cargo run -p arxiv-cli -- store tag-remove http://arxiv.org/abs/1706.03762v1 favorite
```

### `sync`（增量同步）

```bash
cargo run -p arxiv-cli -- sync oai --metadata-prefix oai_dc --state-key arxiv-oai
cargo run -p arxiv-cli -- sync oai --metadata-prefix oai_dc --from 2024-01-01 --until 2024-01-31 --state-key jan-2024
```

当前同步行为：

- 维护 checkpoint（`resumptionToken` + 最近处理记录）
- 当前版本以“状态推进”为主，尚未将 OAI 记录全文写入 `papers` 表

## 限流与 robots.txt

已采用保守策略：

- 参考 `arxiv.org/robots.txt` 中 `Crawl-delay: 15`
- 命中 `429` 后指数退避并解析 `Retry-After`

建议设置联系邮箱：

```bash
export ARXIV_CONTACT_EMAIL="you@example.com"
```

可选端点覆盖：

- `ARXIV_API_BASE_URL`
- `ARXIV_PDF_BASE_URL`
- `ARXIV_SOURCE_BASE_URL`
