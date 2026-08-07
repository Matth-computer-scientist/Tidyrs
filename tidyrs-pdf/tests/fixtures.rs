use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_pdf::PdfParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/pdf").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name} (run `cargo run -p tidyrs-pdf --example gen_fixtures_pdf`): {e}"))
}

#[test]
fn title_line_above_ragged_data_does_not_lose_rows_or_the_header() {
    // Regression (found via external QA testing, not the automated
    // suite): find_header_offset's search used to be able to skip up to
    // 3 leading lines, picking whichever skip level scored the most
    // inferred columns. Its scoring threshold is a *fraction of however
    // many rows remain in the slice being scored* — so on a table with a
    // title line above ragged data (some cells legitimately blank),
    // skipping further wasn't just discarding candidate title lines, it
    // was also shrinking the sample the agreement threshold was measured
    // against, making that threshold trivially easier to clear. That let
    // a 3-line skip (title + real header + the first real data row) win
    // outright, permanently losing a whole data row and misreading a
    // second data row as the table's header. Capping how far this search
    // is even allowed to look (see MAX_TITLE_SKIP in lib.rs) bounds the
    // damage: every real product row must survive, whether or not the
    // title line itself gets correctly identified and stripped.
    let bytes = fixture("title_with_ragged_data.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "title_with_ragged_data.pdf", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    let all_cells: Vec<String> = table.rows.iter().flatten().map(|v| format!("{v:?}")).collect();
    let joined = all_cells.join(" ");
    for expected in [
        "SKU-001",
        "SKU-002",
        "SKU-003",
        "SKU-004",
        "Casque Audio Bluetooth Pro",
        "Chaise de Bureau Ergonomique",
    ] {
        assert!(
            joined.contains(expected),
            "expected to find {expected:?} somewhere in the parsed table, got rows: {:?}",
            table.rows
        );
    }
}

#[test]
fn a_title_the_heuristic_cannot_detect_still_does_not_lose_data() {
    // Known limitation, not a regression to fix: a multi-word title
    // ("Rapport Ventes - Janvier 2026") can score *more* inferred columns
    // when included than the real table scores without it, since the
    // title's own internal word gaps coincidentally subdivide a region
    // the table only sees as one wide gap — the opposite of what the
    // title-skip search assumes. See the extended discussion on
    // find_header_offset in lib.rs for why this isn't patched further:
    // every attempted fix broke real headers in other, more common
    // fixtures. This test exists to pin down that the failure stays
    // *bounded*: the title survives as extra ambiguous columns, but every
    // real data row and value must still come through correctly.
    let bytes = fixture("title_with_no_ragged_data.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "title_with_no_ragged_data.pdf", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    // 4, not 3: the title being merged into the header (the known
    // limitation this test documents) also means the *real* header row
    // ("region", "units", "revenue") gets misread as a fourth data row —
    // a real, visible-on-inspection quality issue, but every actual data
    // value is still present and correct, which is what matters most for
    // a pipeline tool: garbled structure is reviewable, silently missing
    // data is not.
    assert_eq!(
        table.rows.len(),
        4,
        "all 3 real data rows (plus the misread header row) must survive, got rows: {:?}",
        table.rows
    );
    let all_cells: Vec<String> = table.rows.iter().flatten().map(|v| format!("{v:?}")).collect();
    let joined = all_cells.join(" ");
    for expected in ["North", "South", "East", "120", "3120", "8899.99"] {
        assert!(
            joined.contains(expected),
            "expected to find {expected:?} somewhere in the parsed table, got rows: {:?}",
            table.rows
        );
    }
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
fn rows_in_excludes_title_lines_and_the_header_line() {
    // Regression: rows_in used to count every extracted text line,
    // including title lines skipped by find_header_offset and the header
    // line itself, so it never matched rows_out for the same table.
    let bytes = fixture("simple_table.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "simple_table.pdf", &ParseOptions::new()).unwrap();

    assert_eq!(outcome.report.rows_in, 3);
    assert_eq!(outcome.report.rows_out, 3);
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
    let outcome = parser
        .parse(&bytes, "proportional_font_per_field_table.pdf", &ParseOptions::new())
        .unwrap();
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
