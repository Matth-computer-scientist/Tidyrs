//! Calibration/regression suite for format *detection* itself (as
//! opposed to parsing correctness, covered elsewhere): runs
//! `FormatRegistry::detect` against every fixture committed to the repo
//! — both with and without a filename hint — and asserts each one is
//! classified as the format it actually is. This is what "improve
//! detection" has to be measured against instead of guessing: a change
//! to a `sniff()` scoring formula is only a real improvement if it keeps
//! (or grows) the set of fixtures below passing, not just intuitively
//! "feels more principled."

use std::path::{Path, PathBuf};
use tidyrs_core::FormatRegistry;

fn build_registry() -> FormatRegistry {
    let mut reg = FormatRegistry::new();
    reg.register(Box::new(tidyrs_csv::CsvParser::new()));
    reg.register(Box::new(tidyrs_xlsx::XlsxParser::new()));
    reg.register(Box::new(tidyrs_json::JsonXmlParser::new()));
    reg.register(Box::new(tidyrs_fixed::FixedWidthParser::new()));
    reg.register(Box::new(tidyrs_pdf::PdfParser::new()));
    reg.register(Box::new(tidyrs_ini::IniParser::new()));
    reg.register(Box::new(tidyrs_sqlite::SqliteParser::new()));
    reg.register(Box::new(tidyrs_orc::OrcParser::new()));
    reg.register(Box::new(tidyrs_parquet::ParquetParser::new()));
    reg.register(Box::new(tidyrs_avro::AvroParser::new()));
    reg
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures")
}

/// (relative path under fixtures/, expected format_name)
const CASES: &[(&str, &str)] = &[
    ("csv/ambiguous_column_types.csv", "csv"),
    ("csv/comma_quoted_extra.csv", "csv"),
    ("csv/pipe_delimited.csv", "csv"),
    ("csv/quoted_commas_confuse_naive_sniffing.csv", "csv"),
    ("csv/semicolon_ragged.csv", "csv"),
    ("csv/tab_missing_cols.csv", "csv"),
    ("fixed/aligned_columns.txt", "fixed"),
    ("fixed/numeric_report.txt", "fixed"),
    ("fixed/server_log.log", "fixed"),
    ("json/inconsistent_types.json", "json"),
    ("json/orders_with_line_items.json", "json"),
    ("json/single_object.json", "json"),
    ("json/wrapped_items.json", "json"),
    ("xml/products.xml", "json"),          // JsonXmlParser handles both JSON and XML
    ("yaml/list_of_records.yaml", "json"), // JsonXmlParser handles JSON/XML/YAML
    ("yaml/single_object.yaml", "json"),
    ("yaml/wrapped_items.yaml", "json"),
    ("yaml/inconsistent_types.yaml", "json"),
    ("ini/database.ini", "ini"),
    ("ini/simple.ini", "ini"),
    ("env/app.env", "ini"), // IniParser handles both .ini and .env
    ("sqlite/single_table.db", "sqlite"),
    ("sqlite/multi_table.db", "sqlite"),
    ("orc/alltypes.snappy.orc", "orc"),
    ("orc/nested_struct.orc", "orc"),
    ("parquet/users.parquet", "parquet"),
    ("avro/users.avro", "avro"),
    ("pdf/product_table.pdf", "pdf"),
    ("pdf/proportional_font_per_field_table.pdf", "pdf"),
    ("pdf/simple_table.pdf", "pdf"),
    ("pdf/table_with_title.pdf", "pdf"),
    ("xlsx/junk_rows_and_merged_cells.xlsx", "xlsx"),
    ("xlsx/leading_blank_and_title_rows.xlsx", "xlsx"),
    ("xlsx/multi_sheet_different_shapes.xlsx", "xlsx"),
    ("xlsx/single_column_sheet.xlsx", "xlsx"),
    ("xlsx/two_merges_with_gap_between.xlsx", "xlsx"),
    ("real_world/orders_nested.json", "json"),
    ("real_world/q4_financial_report.xlsx", "xlsx"),
    ("real_world/sales_export_messy.csv", "csv"),
    ("real_world/server_activity.log", "fixed"),
    ("real_world/accounts_export.yaml", "json"), // JsonXmlParser handles JSON/XML/YAML
    ("real_world/services.ini", "ini"),
    ("real_world/deploy.env", "ini"),
    ("real_world/shop.db", "sqlite"),
];

#[test]
fn every_committed_fixture_is_detected_correctly_with_filename_hint() {
    let registry = build_registry();
    let mut failures = Vec::new();

    for (rel_path, expected) in CASES {
        let path = fixtures_root().join(rel_path);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {rel_path}: {e}"));
        let filename = Path::new(rel_path).file_name().unwrap().to_str().unwrap();

        match registry.detect(&bytes, Some(filename)) {
            Some(d) if d.parser.format_name() == *expected => {}
            Some(d) => failures.push(format!(
                "{rel_path}: expected '{expected}', got '{}' (confidence {:.2})",
                d.parser.format_name(),
                d.confidence
            )),
            None => failures.push(format!("{rel_path}: expected '{expected}', got no detection at all")),
        }
    }

    assert!(failures.is_empty(), "detection mismatches (with filename hint):\n{}", failures.join("\n"));
}

#[test]
fn every_committed_fixture_is_detected_correctly_from_content_alone() {
    // No filename hint at all — proves detection is genuinely
    // content-based, not secretly relying on the extension.
    let registry = build_registry();
    let mut failures = Vec::new();

    for (rel_path, expected) in CASES {
        let path = fixtures_root().join(rel_path);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {rel_path}: {e}"));

        match registry.detect(&bytes, None) {
            Some(d) if d.parser.format_name() == *expected => {}
            Some(d) => failures.push(format!(
                "{rel_path}: expected '{expected}', got '{}' (confidence {:.2})",
                d.parser.format_name(),
                d.confidence
            )),
            None => failures.push(format!("{rel_path}: expected '{expected}', got no detection at all")),
        }
    }

    assert!(
        failures.is_empty(),
        "detection mismatches (content only, no filename):\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_real_csv_table_past_the_first_4kb_is_still_detected() {
    // sniff() only reads a bounded prefix of the file for performance —
    // but a real export can plausibly have several KB of preamble
    // (a comment block, a metadata section, an unrelated free-text
    // column padded wide) before the actual tabular content starts. This
    // builds exactly that: ~4.3KB of non-tabular prose followed by a
    // small, unambiguous CSV table, and proves detection still finds it
    // rather than only ever looking at the first 4096 bytes.
    let mut bytes = Vec::new();
    // Real prose, unlike tabular/log data, has a *varying* word count
    // line to line — that variation (not just "the file contains
    // sentences") is part of what should tell detection this isn't
    // fixed-width data.
    let padding_lines = [
        "This file was exported from the legacy reporting system.\n",
        "Nobody remembers why it still runs on a Tuesday.\n",
        "Please do not edit the header block below.\n",
        "Contact IT if this export looks wrong.\n",
    ];
    let mut i = 0;
    while bytes.len() < 4300 {
        bytes.extend_from_slice(padding_lines[i % padding_lines.len()].as_bytes());
        i += 1;
    }
    bytes.extend_from_slice(b"id,name,amount\n");
    for i in 1..=12 {
        bytes.extend_from_slice(format!("{i},Person{i},{}.50\n", i * 10).as_bytes());
    }

    let registry = build_registry();
    let detection = registry.detect(&bytes, None);
    match detection {
        Some(d) => assert_eq!(
            d.parser.format_name(),
            "csv",
            "expected csv, got '{}' (confidence {:.2})",
            d.parser.format_name(),
            d.confidence
        ),
        None => panic!("expected the trailing CSV table to be detected even though it starts past the first 4KB"),
    }
}

#[test]
fn a_log_file_with_colon_separated_timestamps_is_not_misdetected_as_yaml() {
    // YAML has no unique leading character the way JSON/XML do, so its
    // content-only detection has to lean on a `key: value` line-shape scan
    // — exactly the shape a "HH:MM:SS" timestamp or a "field: value" log
    // line can superficially resemble. This is what the fixed-width
    // fixture already committed under fixtures/fixed/server_log.log
    // exists to guard against (it's covered by the full-fixture-sweep
    // tests above too), but a synthetic case with timestamps that contain
    // literal colons makes the specific risk explicit.
    let bytes = b"2026-08-01 09:15:02 INFO started worker pool with 4 threads\n\
2026-08-01 09:15:03 INFO listening on port 8080\n\
2026-08-01 09:15:10 WARN slow query took 800ms\n\
2026-08-01 09:15:12 ERROR connection reset by peer\n\
2026-08-01 09:15:15 INFO request completed in 12ms\n";

    let registry = build_registry();
    let detection = registry.detect(bytes, None);
    if let Some(d) = detection {
        assert_ne!(
            d.parser.format_name(),
            "json",
            "timestamp-heavy log lines should not be misdetected as YAML (JsonXmlParser reports as 'json'), \
             got confidence {:.2}",
            d.confidence
        );
    }
}

#[test]
fn a_genuine_yaml_mapping_is_detected_from_content_alone_over_csv_and_fixed() {
    let bytes = b"- name: Alice\n  role: admin\n  active: true\n- name: Bob\n  role: viewer\n  active: false\n";
    let registry = build_registry();
    let detection = registry.detect(bytes, None);
    match detection {
        Some(d) => assert_eq!(
            d.parser.format_name(),
            "json",
            "expected the YAML-handling parser (reports as 'json'), got '{}' (confidence {:.2})",
            d.parser.format_name(),
            d.confidence
        ),
        None => panic!("expected a genuine YAML mapping to be detected from content alone"),
    }
}

#[test]
fn a_genuine_multi_section_ini_is_detected_from_content_alone() {
    let bytes = b"[default]\nhost=localhost\nport=5432\n\n[production]\nhost=db.example.com\nport=5432\n";
    let registry = build_registry();
    let detection = registry.detect(bytes, None);
    match detection {
        Some(d) => assert_eq!(
            d.parser.format_name(),
            "ini",
            "expected ini, got '{}' (confidence {:.2})",
            d.parser.format_name(),
            d.confidence
        ),
        None => panic!("expected a genuine multi-section INI file to be detected from content alone"),
    }
}

#[test]
fn a_url_with_query_parameters_is_not_misdetected_as_ini() {
    // A raw `key=value` scan alone would treat every `&`-joined query
    // parameter as another INI key=value line. IniParser's is_kv_line
    // rejects this because the "key" preceding `=` (e.g. the whole path)
    // contains characters (`/`, `?`) a real config key never would.
    let bytes = b"GET /search?q=rust&page=2&sort=recent HTTP/1.1\n\
Host: example.com\n\
User-Agent: curl/8.0\n";
    let registry = build_registry();
    let detection = registry.detect(bytes, None);
    if let Some(d) = detection {
        assert_ne!(
            d.parser.format_name(),
            "ini",
            "an HTTP request dump should not be misdetected as INI, got confidence {:.2}",
            d.confidence
        );
    }
}
