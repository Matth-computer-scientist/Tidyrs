use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_fixed::FixedWidthParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/fixed").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name}: {e}"))
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
