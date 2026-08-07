use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_sqlite::SqliteParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sqlite").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name} (run `cargo run -p tidyrs-sqlite --example gen_fixtures`): {e}"))
}

fn col<'a>(headers: &[String], row: &'a [TidyValue], name: &str) -> &'a TidyValue {
    let idx = headers
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("no column '{name}' in {headers:?}"));
    &row[idx]
}

#[test]
fn single_table_database_is_read_with_native_column_types() {
    let bytes = fixture("single_table.db");
    let parser = SqliteParser::new();
    let outcome = parser.parse(&bytes, "single_table.db", &ParseOptions::new()).unwrap();

    assert_eq!(outcome.tables.len(), 1);
    let table = &outcome.tables[0];
    assert_eq!(table.rows.len(), 3);
    assert_eq!(col(&table.headers, &table.rows[0], "name"), &TidyValue::Text("Alice".to_string()));
    assert_eq!(col(&table.headers, &table.rows[0], "age"), &TidyValue::Int(30));
    assert_eq!(col(&table.headers, &table.rows[0], "balance"), &TidyValue::Float(120.5));
    // SQLite NULL must round-trip as TidyValue::Null, not a stringified "NULL".
    assert_eq!(col(&table.headers, &table.rows[1], "age"), &TidyValue::Null);
    assert_eq!(outcome.report.rows_in, 3);
    assert_eq!(outcome.report.rows_out, 3);
}

#[test]
fn multi_table_database_produces_one_table_per_sql_table() {
    let bytes = fixture("multi_table.db");
    let parser = SqliteParser::new();
    let outcome = parser.parse(&bytes, "multi_table.db", &ParseOptions::new()).unwrap();

    assert_eq!(outcome.tables.len(), 2);
    let names: Vec<&str> = outcome.tables.iter().filter_map(|t| t.source.as_deref()).collect();
    assert!(names.contains(&"customers"));
    assert!(names.contains(&"orders"));

    let customers = outcome.tables.iter().find(|t| t.source.as_deref() == Some("customers")).unwrap();
    assert_eq!(customers.rows.len(), 2);
    let orders = outcome.tables.iter().find(|t| t.source.as_deref() == Some("orders")).unwrap();
    assert_eq!(orders.rows.len(), 3);
    assert_eq!(col(&orders.headers, &orders.rows[0], "total"), &TidyValue::Float(250.0));
}

#[test]
fn table_option_restricts_to_one_table() {
    let bytes = fixture("multi_table.db");
    let parser = SqliteParser::new();
    let opts = ParseOptions::new().set("table", "orders");
    let outcome = parser.parse(&bytes, "multi_table.db", &opts).unwrap();

    assert_eq!(outcome.tables.len(), 1);
    assert_eq!(outcome.tables[0].source.as_deref(), Some("orders"));
}

#[test]
fn sniff_recognizes_the_sqlite_magic_header() {
    let bytes = fixture("single_table.db");
    let parser = SqliteParser::new();
    assert!(parser.sniff(&bytes, Some("single_table.db")) > 0.9);
    // Content alone, no filename hint at all.
    assert!(parser.sniff(&bytes, None) > 0.7);
}
