use std::fs;

use assert_cmd::Command;
use httpmock::Method::GET;
use httpmock::MockServer;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn help_shows_main_commands() {
    let mut cmd = Command::cargo_bin("arxiv-cli").expect("binary should be built");

    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("arxiv").and(predicate::str::contains("oai")));
}

#[test]
fn subcommand_help_shows_expected_actions() {
    let mut arxiv_help = Command::cargo_bin("arxiv-cli").expect("binary should be built");
    arxiv_help
        .args(["arxiv", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("search")
                .and(predicate::str::contains("show"))
                .and(predicate::str::contains("download")),
        );

    let mut oai_help = Command::cargo_bin("arxiv-cli").expect("binary should be built");
    oai_help.args(["oai", "--help"]).assert().success().stdout(
        predicate::str::contains("identify")
            .and(predicate::str::contains("list-records"))
            .and(predicate::str::contains("list-identifiers"))
            .and(predicate::str::contains("get-record")),
    );
}

#[test]
fn search_without_query_returns_validation_error() {
    let mut cmd = Command::cargo_bin("arxiv-cli").expect("binary should be built");

    cmd.args(["arxiv", "search"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("至少提供一个查询条件"));
}

#[test]
fn download_refuses_existing_file_without_force() {
    let tmp_dir = tempdir().expect("create temp dir");
    let output = tmp_dir.path().join("paper.pdf");
    fs::write(&output, b"already there").expect("write fixture");

    let mut cmd = Command::cargo_bin("arxiv-cli").expect("binary should be built");
    cmd.args([
        "arxiv",
        "download",
        "1706.03762",
        "--output",
        output.to_str().expect("valid utf-8 path"),
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("目标文件已存在"));
}

#[test]
fn search_all_results_reuses_paging_like_arxiv_py() {
    let server = MockServer::start();

    let page1 = feed_xml(
        3,
        0,
        2,
        vec![
            ("1111.00001v1", "Paper One", "First abstract"),
            ("1111.00002v1", "Paper Two", "Second abstract"),
        ],
    );
    let page2 = feed_xml(
        3,
        2,
        1,
        vec![("1111.00003v1", "Paper Three", "Third abstract")],
    );

    let _first = server.mock(|when, then| {
        when.method(GET)
            .path("/api/query")
            .query_param("start", "0")
            .query_param("max_results", "2");
        then.status(200)
            .header("content-type", "application/atom+xml")
            .body(page1);
    });

    let _second = server.mock(|when, then| {
        when.method(GET)
            .path("/api/query")
            .query_param("start", "2")
            .query_param("max_results", "1");
        then.status(200)
            .header("content-type", "application/atom+xml")
            .body(page2);
    });

    let mut cmd = Command::cargo_bin("arxiv-cli").expect("binary should be built");
    cmd.env(
        "ARXIV_API_BASE_URL",
        format!("{}/api/query", server.base_url()),
    )
    .args([
        "arxiv",
        "search",
        "--query",
        "transformer",
        "--all-results",
        "--limit",
        "3",
        "--batch-size",
        "2",
        "--format",
        "json",
    ])
    .assert()
    .success()
    .stdout(
        predicate::str::contains("\"total_results\": 3")
            .and(predicate::str::contains("\"short_id\": \"1111.00001v1\""))
            .and(predicate::str::contains("\"short_id\": \"1111.00003v1\"")),
    );
}

#[test]
fn search_all_results_rejects_zero_batch_size() {
    let mut cmd = Command::cargo_bin("arxiv-cli").expect("binary should be built");

    cmd.args([
        "arxiv",
        "search",
        "--query",
        "transformer",
        "--all-results",
        "--batch-size",
        "0",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("batch_size 必须大于 0"));
}

#[test]
fn oai_identify_works() {
    let server = MockServer::start();

    let identify_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/">
  <Identify>
    <repositoryName>Test Repo</repositoryName>
    <baseURL>http://example.test/oai2</baseURL>
    <protocolVersion>2.0</protocolVersion>
    <adminEmail>admin@example.test</adminEmail>
    <earliestDatestamp>2020-01-01</earliestDatestamp>
    <deletedRecord>no</deletedRecord>
    <granularity>YYYY-MM-DD</granularity>
  </Identify>
</OAI-PMH>"#;

    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/oai2")
            .query_param("verb", "Identify");
        then.status(200)
            .header("content-type", "text/xml")
            .body(identify_xml);
    });

    let mut cmd = Command::cargo_bin("arxiv-cli").expect("binary should be built");
    cmd.args([
        "oai",
        "identify",
        "--base-url",
        &format!("{}/oai2", server.base_url()),
        "--format",
        "json",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains(
        "\"repository_name\": \"Test Repo\"",
    ));
}

#[test]
fn oai_list_identifiers_works() {
    let server = MockServer::start();

    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/">
  <ListIdentifiers>
    <header>
      <identifier>oai:arXiv.org:1111.00001</identifier>
      <datestamp>2026-03-31</datestamp>
      <setSpec>cs</setSpec>
    </header>
  </ListIdentifiers>
</OAI-PMH>"#;

    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/oai2")
            .query_param("verb", "ListIdentifiers")
            .query_param("metadataPrefix", "oai_dc");
        then.status(200)
            .header("content-type", "text/xml")
            .body(xml);
    });

    let mut cmd = Command::cargo_bin("arxiv-cli").expect("binary should be built");
    cmd.args([
        "oai",
        "list-identifiers",
        "--base-url",
        &format!("{}/oai2", server.base_url()),
        "--format",
        "json",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains(
        "\"identifier\": \"oai:arXiv.org:1111.00001\"",
    ));
}

#[test]
fn oai_get_record_works() {
    let server = MockServer::start();

    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<OAI-PMH xmlns="http://www.openarchives.org/OAI/2.0/">
  <GetRecord>
    <record>
      <header>
        <identifier>oai:arXiv.org:1234.5678</identifier>
        <datestamp>2026-03-31</datestamp>
      </header>
      <metadata>
        <dc>
          <title>Mock Record</title>
        </dc>
      </metadata>
    </record>
  </GetRecord>
</OAI-PMH>"#;

    let _mock = server.mock(|when, then| {
        when.method(GET)
            .path("/oai2")
            .query_param("verb", "GetRecord")
            .query_param("identifier", "oai:arXiv.org:1234.5678")
            .query_param("metadataPrefix", "oai_dc");
        then.status(200)
            .header("content-type", "text/xml")
            .body(xml);
    });

    let mut cmd = Command::cargo_bin("arxiv-cli").expect("binary should be built");
    cmd.args([
        "oai",
        "get-record",
        "--identifier",
        "oai:arXiv.org:1234.5678",
        "--base-url",
        &format!("{}/oai2", server.base_url()),
        "--format",
        "json",
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains(
        "\"identifier\": \"oai:arXiv.org:1234.5678\"",
    ));
}

fn feed_xml(
    total: usize,
    start: usize,
    page_size: usize,
    entries: Vec<(&str, &str, &str)>,
) -> String {
    let mut body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\" xmlns:opensearch=\"http://a9.com/-/spec/opensearch/1.1/\" xmlns:arxiv=\"http://arxiv.org/schemas/atom\">\n  <opensearch:totalResults>{total}</opensearch:totalResults>\n  <opensearch:startIndex>{start}</opensearch:startIndex>\n  <opensearch:itemsPerPage>{page_size}</opensearch:itemsPerPage>\n"
    );

    for (id, title, summary) in entries {
        body.push_str(&format!(
            "  <entry>\n    <id>http://arxiv.org/abs/{id}</id>\n    <updated>2024-01-01T00:00:00Z</updated>\n    <published>2024-01-01T00:00:00Z</published>\n    <title>{title}</title>\n    <summary>{summary}</summary>\n    <author><name>Test Author</name></author>\n    <link title=\"pdf\" href=\"http://arxiv.org/pdf/{id}\" rel=\"related\" type=\"application/pdf\" />\n    <arxiv:primary_category term=\"cs.CL\" scheme=\"http://arxiv.org/schemas/atom\" />\n    <category term=\"cs.CL\" scheme=\"http://arxiv.org/schemas/atom\" />\n  </entry>\n"
        ));
    }

    body.push_str("</feed>");
    body
}
