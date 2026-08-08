use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};
use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_xlsx::XlsxParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/xlsx").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name} (run `cargo run -p tidyrs-xlsx --example gen_fixtures_xlsx`): {e}"))
}

// Regressions found via external QA testing, all rooted in the same
// mistake: cell_to_tidy used to call calamine's as_i64()/as_f64(), which
// silently *coerce* across cell types (parsing a String cell as an int,
// saturating-casting an oversized Float to an int) rather than reporting
// what the cell's real stored type actually is. Built in-code via
// rust_xlsxwriter rather than a committed binary fixture, matching the
// pattern already used for tidyrs-avro/tidyrs-pdf's own narrow,
// mechanism-specific regression tests.

#[test]
fn a_text_formatted_cell_with_a_leading_zero_is_not_silently_converted_to_a_number() {
    // Data::String("007").as_i64() used to return Some(7) — calamine's
    // own as_i64() parses strings as a convenience, discarding the exact
    // reason the source spreadsheet stored it as text in the first place.
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, "code").unwrap();
    ws.write_string(1, 0, "007").unwrap();
    let bytes = wb.save_to_buffer().unwrap();

    let parser = XlsxParser::new();
    let outcome = parser.parse(&bytes, "codes.xlsx", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows[0][0], TidyValue::Text("007".to_string()), "got: {:?}", table.rows[0]);
}

#[test]
fn a_huge_float_does_not_saturate_to_i64_max() {
    // Data::Float(v).as_i64() used to do `v as i64` — Rust's `as` cast
    // *saturates* rather than erroring on overflow, so a cell holding
    // 1e300 silently became i64::MAX (9223372036854775807): not rounded,
    // a completely different, wrong number with zero signal anything
    // went wrong.
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, "value").unwrap();
    ws.write_number(1, 0, 1e300).unwrap();
    let bytes = wb.save_to_buffer().unwrap();

    let parser = XlsxParser::new();
    let outcome = parser.parse(&bytes, "overflow.xlsx", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows[0][0], TidyValue::Float(1e300), "got: {:?}", table.rows[0]);
}

#[test]
fn a_date_formatted_cell_reads_as_a_real_date_not_a_raw_serial_number() {
    // calamine's "dates" cargo feature wasn't enabled at all, so every
    // date-formatted cell came through as its raw, meaningless Excel
    // serial number (e.g. 46027) instead of a real calendar date.
    //
    // A cell only reads back as a date if its stored number format is one
    // Excel/calamine recognize as a date format — write_datetime() alone
    // (no explicit Format) leaves the cell in the default "General"
    // format, indistinguishable from an ordinary number, so this test
    // must apply one explicitly the way a real spreadsheet's date column
    // always does.
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let date_format = Format::new().set_num_format("yyyy-mm-dd");
    ws.write_string(0, 0, "signup_date").unwrap();
    ws.write_datetime_with_format(1, 0, ExcelDateTime::from_ymd(2026, 1, 15).unwrap(), &date_format)
        .unwrap();
    let bytes = wb.save_to_buffer().unwrap();

    let parser = XlsxParser::new();
    let outcome = parser.parse(&bytes, "dates.xlsx", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    match &table.rows[0][0] {
        TidyValue::Text(s) => assert!(s.contains("2026-01-15"), "expected a real calendar date, got {s:?}"),
        other => panic!("expected a Text date value, got {other:?}"),
    }
}

#[test]
fn single_column_sheet_keeps_all_its_data_rows() {
    // Regression: a legitimately single-column sheet used to have every
    // data row misread as footer junk (each row has exactly 1 populated
    // cell, the same shape the footer heuristic watches for), leaving
    // only the header behind.
    let bytes = fixture("single_column_sheet.xlsx");
    let parser = XlsxParser::new();
    let outcome = parser.parse(&bytes, "single_column_sheet.xlsx", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["note"]);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0][0], TidyValue::Text("First observation about the dataset.".to_string()));
    assert_eq!(table.rows[2][0], TidyValue::Text("Third and final note.".to_string()));
}

#[test]
fn merged_cells_are_forward_filled_and_junk_rows_trimmed() {
    let bytes = fixture("junk_rows_and_merged_cells.xlsx");
    let parser = XlsxParser::new();
    let outcome = parser.parse(&bytes, "junk_rows_and_merged_cells.xlsx", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["name", "score", "team"]);
    // The title row and the footer row must not have become data rows.
    assert_eq!(table.rows.len(), 3);
    // The merged "team" cell (Blue, spanning rows 2-4) must be forward-filled
    // into every row of the merged region, not just the first.
    assert_eq!(table.rows[0][2], TidyValue::Text("Blue".to_string()));
    assert_eq!(table.rows[1][2], TidyValue::Text("Blue".to_string()));
    assert_eq!(table.rows[2][2], TidyValue::Text("Blue".to_string()));
}

#[test]
fn multi_sheet_workbook_produces_one_table_per_sheet_with_own_shape() {
    let bytes = fixture("multi_sheet_different_shapes.xlsx");
    let parser = XlsxParser::new();
    let outcome = parser.parse(&bytes, "multi_sheet_different_shapes.xlsx", &ParseOptions::new()).unwrap();

    assert_eq!(outcome.tables.len(), 2);
    let people = outcome.tables.iter().find(|t| t.source.as_deref() == Some("People")).unwrap();
    let orders = outcome.tables.iter().find(|t| t.source.as_deref() == Some("Orders")).unwrap();

    assert_eq!(people.headers, vec!["name", "age"]);
    assert_eq!(orders.headers, vec!["product", "price", "qty"]);
    assert_eq!(people.rows.len(), 2);
    assert_eq!(orders.rows.len(), 2);
}

#[test]
fn leading_blank_and_title_rows_are_skipped() {
    let bytes = fixture("leading_blank_and_title_rows.xlsx");
    let parser = XlsxParser::new();
    let outcome = parser.parse(&bytes, "leading_blank_and_title_rows.xlsx", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["sku", "description", "in_stock"]);
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0][2], TidyValue::Bool(true));
}

#[test]
fn exact_merge_regions_do_not_leak_into_unmerged_gaps() {
    // A naive column forward-fill would incorrectly propagate "Blue" into
    // Carla's row (blank but NOT part of any merge). Exact merge-region
    // boundaries must leave it Null and correctly fill only Dave/Eve with
    // the second, separate "Red" merge.
    let bytes = fixture("two_merges_with_gap_between.xlsx");
    let parser = XlsxParser::new();
    let outcome = parser.parse(&bytes, "two_merges_with_gap_between.xlsx", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["name", "team"]);
    assert_eq!(table.rows.len(), 5);
    assert_eq!(table.rows[0][1], TidyValue::Text("Blue".to_string())); // Alice
    assert_eq!(table.rows[1][1], TidyValue::Text("Blue".to_string())); // Bob
    assert_eq!(table.rows[2][1], TidyValue::Null); // Carla - not merged, must stay empty
    assert_eq!(table.rows[3][1], TidyValue::Text("Red".to_string())); // Dave
    assert_eq!(table.rows[4][1], TidyValue::Text("Red".to_string())); // Eve

    assert!(outcome.report.notes.iter().any(|n| n.message.contains("exact boundaries")));
}

#[test]
fn sheet_option_restricts_to_one_sheet() {
    let bytes = fixture("multi_sheet_different_shapes.xlsx");
    let parser = XlsxParser::new();
    let opts = ParseOptions::new().set("sheet", "Orders");
    let outcome = parser.parse(&bytes, "multi_sheet_different_shapes.xlsx", &opts).unwrap();

    assert_eq!(outcome.tables.len(), 1);
    assert_eq!(outcome.tables[0].source.as_deref(), Some("Orders"));
}
