use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_fixed::FixedWidthParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/fixed").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name}: {e}"))
}

#[test]
fn a_repeated_whitespace_mode_header_name_is_disambiguated() {
    // Regression (found via manual QA testing): a source file's own
    // header line repeating a name used to pass straight through
    // unchanged. Same fix applied to tidyrs-csv/tidyrs-xlsx/tidyrs-pdf.
    let bytes = b"id id name\n1 2 Bob\n".to_vec();
    let parser = FixedWidthParser::new();
    let opts = ParseOptions::new().set("mode", "whitespace").set("has_header", "true");
    let outcome = parser.parse(&bytes, "dup.txt", &opts).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["id", "id_2", "name"]);
}

#[test]
fn rows_in_excludes_the_header_row() {
    // Regression: rows_in used to count the header as a data row in both
    // the "fixed" and "whitespace" modes.
    let bytes = fixture("aligned_columns.txt");
    let parser = FixedWidthParser::new();
    let opts = ParseOptions::new().set("has_header", "true");
    let outcome = parser.parse(&bytes, "aligned_columns.txt", &opts).unwrap();

    assert_eq!(outcome.report.rows_in, 3);
    assert_eq!(outcome.report.rows_out, 3);
}

#[test]
fn sniff_rejects_content_that_is_mostly_control_characters() {
    // Same class of bug as tidyrs-csv's sniff: a deterministic "looks
    // binary" buffer must never score high enough to be misdetected.
    let mut junk = vec![0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
    junk.extend([b'\n']);
    junk.extend([0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    junk.extend([b'\n']);
    junk.extend(vec![0x01u8; 100]);

    let parser = FixedWidthParser::new();
    assert_eq!(parser.sniff(&junk, Some("mystery.txt")), 0.0);
}

#[test]
fn aligned_columns_are_inferred_with_header() {
    let bytes = fixture("aligned_columns.txt");
    let parser = FixedWidthParser::new();
    let opts = ParseOptions::new().set("has_header", "true");
    let outcome = parser.parse(&bytes, "aligned_columns.txt", &opts).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["name", "age", "city"]);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0][0], TidyValue::Text("Alice".to_string()));
    assert_eq!(table.rows[0][1], TidyValue::Int(30));
    assert_eq!(table.rows[2][2], TidyValue::Text("Marseille".to_string()));
}

#[test]
fn numeric_fixed_width_columns_get_typed() {
    let bytes = fixture("numeric_report.txt");
    let parser = FixedWidthParser::new();
    let opts = ParseOptions::new().set("has_header", "true");
    let outcome = parser.parse(&bytes, "numeric_report.txt", &opts).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["region", "units", "revenue"]);
    assert_eq!(table.rows[2][1], TidyValue::Int(210));
    assert_eq!(table.rows[2][2], TidyValue::Float(8899.99));
}

#[test]
fn whitespace_mode_splits_log_lines_into_tokens() {
    let bytes = fixture("server_log.log");
    let parser = FixedWidthParser::new();
    let opts = ParseOptions::new().set("mode", "whitespace");
    let outcome = parser.parse(&bytes, "server_log.log", &opts).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 4);
    // First two tokens on every line are the date and time.
    assert_eq!(table.rows[0][0], TidyValue::Text("2026-01-03".to_string()));
    assert_eq!(table.rows[0][1], TidyValue::Text("08:12:01".to_string()));
    assert_eq!(table.rows[2][2], TidyValue::Text("ERROR".to_string()));
}

#[test]
fn without_header_generic_field_names_are_generated() {
    let bytes = fixture("numeric_report.txt");
    let parser = FixedWidthParser::new();
    let outcome = parser.parse(&bytes, "numeric_report.txt", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers[0], "field_1");
    // With no header, the header text line becomes a data row too.
    assert_eq!(table.rows.len(), 5);
}
