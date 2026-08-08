use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_pdf::PdfParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/pdf").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name} (run `cargo run -p tidyrs-pdf --example gen_fixtures_pdf`): {e}"))
}

fn col<'a>(headers: &[String], row: &'a [TidyValue], name: &str) -> &'a TidyValue {
    let idx = headers
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("no column '{name}' in {headers:?}"));
    &row[idx]
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
fn a_data_cell_that_looks_like_two_words_can_still_cost_the_header() {
    // Known limitation, not a regression to fix: found via external QA
    // testing, worse than the two title-related cases above because the
    // header is lost entirely rather than just merged with extra columns.
    // "product qty amount" is the real header over a table whose product
    // names contain an internal space ("Widget A", "Widget B", "Widget
    // C") and whose qty/amount columns are right-aligned within wide
    // fields. All three data rows happen to share a whitespace gap at the
    // same character position (between the product name and its letter
    // suffix) that the header text doesn't share, so *excluding* the
    // header scores more inferred columns (4) than *including* it (3) —
    // the mirror image of the "Rapport Ventes" title case, where
    // *including* a junk line scored more columns than excluding it. Same
    // root flaw either way: find_header_offset's "more columns wins" rule
    // isn't a safe proxy for "found the real table" in either direction,
    // and every attempted redesign (see the extended discussion on
    // find_header_offset in lib.rs) broke more common cases than it
    // fixed. This test exists to pin down that the failure stays
    // *bounded*: the header is lost and "Widget"/"A" get split into two
    // columns instead of one, but every real product, quantity, and
    // amount value is still present and correct.
    let bytes = fixture("right_aligned_numbers.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "right_aligned_numbers.pdf", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    // 2, not 3: the first data row ("Widget A  8  89.90") is misread as
    // the header and lost from the row count — a real, visible-on-
    // inspection quality issue — but "Widget B" and "Widget C"'s rows
    // must both still come through with every value intact.
    assert_eq!(
        table.rows.len(),
        2,
        "the two data rows not consumed as the (wrong) header must survive, got rows: {:?}",
        table.rows
    );
    let all_cells: Vec<String> = table.rows.iter().flatten().map(|v| format!("{v:?}")).collect();
    let joined = all_cells.join(" ");
    for expected in ["Widget", "B", "12", "45", "C", "3", "249"] {
        assert!(
            joined.contains(expected),
            "expected to find {expected:?} somewhere in the parsed table, got rows: {:?}",
            table.rows
        );
    }
}

#[test]
fn a_table_spanning_two_pages_does_not_merge_rows_across_the_page_break() {
    // Regression (found via external QA testing): glyph rows were
    // clustered purely by Y position with no concept of a page boundary.
    // Each page's own coordinate flip is relative to that page's own
    // media box, so page 2's rows landed at nearly the same (x, y) as
    // page 1's — e.g. both pages naturally start their first row a fixed
    // distance from their own top edge. That let unrelated rows from
    // different pages get merged and their glyphs interleaved
    // character-by-character (a real report this reproduced against
    // showed a header word like "Prix" coming out as "column_3" + "rix").
    // See `glyphs::group_into_rows` for the fix: a page change now forces
    // a new row unconditionally, the same way the Y-tolerance already did
    // for genuinely different lines on one page.
    let bytes = fixture("multi_page_table.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "multi_page_table.pdf", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["sku", "description", "price"]);
    assert_eq!(
        table.rows.len(),
        4,
        "all 4 data rows across both pages must survive, got rows: {:?}",
        table.rows
    );
    assert_eq!(table.rows[0][0], TidyValue::Text("A100".to_string()));
    assert_eq!(table.rows[1][2], TidyValue::Float(14.5));
    // Page 2's rows specifically — the ones that used to get corrupted.
    assert_eq!(
        col(&table.headers, &table.rows[2], "description"),
        &TidyValue::Text("Green Widget".to_string())
    );
    assert_eq!(col(&table.headers, &table.rows[3], "sku"), &TidyValue::Text("A103".to_string()));
    assert_eq!(col(&table.headers, &table.rows[3], "price"), &TidyValue::Float(7.0));
}

#[test]
fn a_free_text_paragraph_below_a_table_loses_no_characters() {
    // Known limitation, not fully fixed: found via external QA testing, a
    // real table followed by a separate free-text "Comments:" paragraph
    // gets column-sliced wherever the table's whitespace alignment
    // happens to land on the paragraph's word-wrapped lines, since there
    // is no "where does the table end" detector (see the module docs in
    // lib.rs — that part is out of scope, same as the find_header_offset
    // limitations elsewhere in this file). What *is* fixed here: a prose
    // character that happens to land exactly on a gap position every
    // table row leaves blank used to be silently dropped rather than
    // merely misplaced (`extract_row` used to be `extract_span` mapped
    // per-column, which has no way to preserve a character between two
    // spans). This test pins down that every character from the
    // paragraph survives *somewhere* in the output, even though which
    // cell it lands in is still unreliable.
    let bytes = fixture("table_with_trailing_comments.pdf");
    let parser = PdfParser::new();
    let outcome = parser.parse(&bytes, "table_with_trailing_comments.pdf", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    // Every real table value must still be exactly right — the paragraph
    // being present at all must not disturb the table rows above it. (The
    // "region"/"units" header text merging into one span here is this
    // fixture's own column-width quirk, unrelated to what this test is
    // pinning down — see the title/right-aligned-numbers tests above for
    // that separate class of limitation.)
    assert_eq!(table.headers[1], "revenue");
    assert_eq!(
        table.rows[0][0],
        TidyValue::Text("North      120".to_string()),
        "got headers {:?}, rows {:?}",
        table.headers,
        table.rows
    );
    assert_eq!(col(&table.headers, &table.rows[2], "revenue"), &TidyValue::Float(8899.99));

    // Concatenate each row's cells *without* a separator (cells are in
    // left-to-right span order, so this reconstructs exactly what a gap-
    // position character being glued onto its neighbor should produce)
    // and confirm no letter from the paragraph went missing anywhere —
    // this is what actually regressed before extract_row: "regions" lost
    // its leading "r" and "underperformed" lost its leading "u" (glued
    // instead onto "region" one cell over), both silently, not just
    // misplaced into the wrong column.
    let per_row_concat: Vec<String> = table
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|v| match v {
                    TidyValue::Text(s) => s.clone(),
                    other => format!("{other:?}"),
                })
                .collect::<String>()
        })
        .collect();
    let joined = per_row_concat.join(" ");
    for expected_word in [
        "Comments",
        "Sales",
        "were",
        "strong",
        "regions",
        "quarter",
        "trimestre",
        "South",
        "underperformed",
        "warehouse",
        "delay",
    ] {
        assert!(
            joined.contains(expected_word),
            "expected {expected_word:?} to survive intact within a single row, got rows: {:?}",
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
