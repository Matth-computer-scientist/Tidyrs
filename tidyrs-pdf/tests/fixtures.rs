use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_pdf::PdfParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/pdf").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name} (run `cargo run -p tidyrs-pdf --example gen_fixtures`): {e}"))
}

#[test]
fn simple_aligned_table_is_reconstructed() {
    let bytes = fixture("simple_table.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "simple_table.pdf", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["name", "age", "city"]);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0][0], TidyValue::Text("Alice".to_string()));
    assert_eq!(table.rows[0][1], TidyValue::Int(30));
    assert_eq!(table.rows[2][2], TidyValue::Text("Marseille".to_string()));

    // Experimental status must be surfaced to the caller, not hidden.
    assert!(outcome.report.notes.iter().any(|n| n.message.contains("experimental")));
}

#[test]
fn title_line_above_the_header_is_detected_and_skipped() {
    let bytes = fixture("table_with_title.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "table_with_title.pdf", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["region", "units", "revenue"]);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[2][2], TidyValue::Float(8899.99));
    assert!(outcome.report.notes.iter().any(|n| n.message.contains("looked like a title")));
}

#[test]
fn product_table_extracts_expected_column_count() {
    let bytes = fixture("product_table.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "product_table.pdf", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers.len(), 3);
    assert_eq!(table.rows.len(), 3);
}

#[test]
fn proportional_font_table_built_from_per_field_text_calls_is_reconstructed() {
    // This is the case the old character-count heuristic explicitly
    // documented as broken: a proportional font (Helvetica) with each
    // cell placed via its own separate text-show call at a fixed x
    // position, rather than one pre-padded monospace string per row.
    // Real-world generated PDFs (invoices, reports) are built this way.
    let bytes = fixture("proportional_font_per_field_table.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "proportional_font_per_field_table.pdf", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["name", "age", "city"]);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0][0], TidyValue::Text("Alice".to_string()));
    assert_eq!(table.rows[0][1], TidyValue::Int(30));
    assert_eq!(table.rows[1][2], TidyValue::Text("Lyon".to_string()));
    assert_eq!(table.rows[2][0], TidyValue::Text("Charlotte".to_string()));
}

#[test]
fn sniff_recognizes_pdf_magic_bytes() {
    let bytes = fixture("simple_table.pdf");
    let parser = PdfParser::new();
    assert!(parser.sniff(&bytes, Some("simple_table.pdf")) > 0.8);
}
