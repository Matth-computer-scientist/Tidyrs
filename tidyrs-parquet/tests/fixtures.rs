use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_parquet::ParquetParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/parquet").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name} (run `cargo run -p tidyrs-parquet --example gen_fixtures_parquet`): {e}"))
}

fn col<'a>(headers: &[String], row: &'a [TidyValue], name: &str) -> &'a TidyValue {
    let idx = headers
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("no column '{name}' in {headers:?}"));
    &row[idx]
}

#[test]
fn primitive_types_and_nulls_are_read_with_correct_native_typing() {
    let bytes = fixture("users.parquet");
    let parser = ParquetParser::new();
    let outcome = parser.parse(&bytes, "users.parquet", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["id", "name", "score", "active", "signup_date"]);
    assert_eq!(table.rows.len(), 5);

    // Int/Float/Boolean map to native TidyValue variants, not Text.
    assert_eq!(col(&table.headers, &table.rows[0], "id"), &TidyValue::Int(1));
    assert_eq!(col(&table.headers, &table.rows[0], "score"), &TidyValue::Float(91.5));
    assert_eq!(col(&table.headers, &table.rows[1], "active"), &TidyValue::Bool(false));

    // A null in any column comes through as TidyValue::Null, whichever
    // column it's in.
    assert_eq!(col(&table.headers, &table.rows[2], "name"), &TidyValue::Null);
    assert_eq!(col(&table.headers, &table.rows[2], "active"), &TidyValue::Null);
    assert_eq!(col(&table.headers, &table.rows[2], "signup_date"), &TidyValue::Null);
    assert_eq!(col(&table.headers, &table.rows[3], "score"), &TidyValue::Null);

    // Date32 renders as a real calendar date via Arrow's own display
    // formatting (the documented simplification — see module docs): day 0
    // is the epoch, and a negative day count (day -1) must resolve to the
    // day *before* the epoch, not panic or produce garbage.
    assert_eq!(
        col(&table.headers, &table.rows[0], "signup_date"),
        &TidyValue::Text("1970-01-01".to_string())
    );
    assert_eq!(
        col(&table.headers, &table.rows[3], "signup_date"),
        &TidyValue::Text("1969-12-31".to_string())
    );

    // Non-ASCII text must survive intact.
    assert_eq!(col(&table.headers, &table.rows[4], "name"), &TidyValue::Text("大熊".to_string()));

    assert_eq!(outcome.report.rows_in, 5);
    assert_eq!(outcome.report.rows_out, 5);
}

#[test]
fn sniff_recognizes_parquet_files_from_content_alone() {
    let bytes = fixture("users.parquet");
    let parser = ParquetParser::new();
    assert!(parser.sniff(&bytes, None) > 0.5);
    assert!(parser.sniff(&bytes, Some("users.parquet")) > 0.8);
}
