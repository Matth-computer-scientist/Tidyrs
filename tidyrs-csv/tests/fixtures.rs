use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_csv::CsvParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/csv").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name}: {e}"))
}

#[test]
fn semicolon_ragged_rows_are_padded_not_dropped() {
    let bytes = fixture("semicolon_ragged.csv");
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "semicolon_ragged.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["name", "age", "city", "notes"]);
    assert_eq!(table.rows.len(), 4);
    for row in &table.rows {
        assert_eq!(row.len(), 4);
    }
    // Bob's row was short (missing notes) -> padded with Null.
    assert_eq!(table.rows[1][3], TidyValue::Null);
    // Charlotte's row had an extra field -> truncated to 4 columns.
    assert_eq!(table.rows[2][0], TidyValue::Text("Charlotte".to_string()));
    assert!(outcome.report.notes.iter().any(|n| n.message.contains("inconsistent column count")));
}

#[test]
fn pipe_delimiter_is_detected_and_types_inferred() {
    let bytes = fixture("pipe_delimited.csv");
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "pipe_delimited.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["id", "product", "price", "in_stock"]);
    assert_eq!(table.rows[0][0], TidyValue::Int(1));
    assert_eq!(table.rows[0][2], TidyValue::Float(9.99));
    assert_eq!(table.rows[0][3], TidyValue::Bool(true));
}

#[test]
fn tab_delimiter_with_missing_and_extra_columns() {
    let bytes = fixture("tab_missing_cols.csv");
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "tab_missing_cols.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers.len(), 4);
    assert_eq!(table.rows.len(), 3);
    // Marie's row was missing the status column.
    assert_eq!(table.rows[1][3], TidyValue::Null);
}

#[test]
fn comma_delimiter_respects_quoted_commas() {
    let bytes = fixture("comma_quoted_extra.csv");
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "comma_quoted_extra.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows[0][1], TidyValue::Text("Blue, Large".to_string()));
    assert_eq!(table.rows[2][2], TidyValue::Null);
}

#[test]
fn one_bad_value_does_not_downgrade_the_whole_numeric_column_to_text() {
    // "age" is mostly integers with one typo ("seventeen"); "score" is
    // mostly integers with one "N/A". Neither should push the resolver
    // to commit the whole column to Text (which would silently turn 30,
    // 41, 25 into strings) — this is exactly the AmbiguityResolver
    // integration's job: recognize genuine ambiguity and fall back to
    // per-cell inference instead of destroying good data.
    let bytes = fixture("ambiguous_column_types.csv");
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "ambiguous_column_types.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows[0][1], TidyValue::Int(30)); // Alice's age
    assert_eq!(table.rows[3][1], TidyValue::Text("seventeen".to_string())); // Dave's age
    assert_eq!(table.rows[0][2], TidyValue::Int(91)); // Alice's score
    assert_eq!(table.rows[1][2], TidyValue::Text("N/A".to_string())); // Bob's score

    assert!(outcome.report.notes.iter().any(|n| n.message.contains("column 'age': type is ambiguous")));
    assert!(outcome
        .report
        .notes
        .iter()
        .any(|n| n.message.contains("column 'score': type is ambiguous")));
}

#[test]
fn quoted_commas_do_not_fool_delimiter_detection() {
    // Every line has exactly two semicolons (the real delimiter) AND
    // exactly two commas hidden inside a quoted field — a naive
    // "just count bytes" sniffer would tie on both and could easily pick
    // the wrong one. Quote-aware counting must still find semicolon.
    let bytes = fixture("quoted_commas_confuse_naive_sniffing.csv");
    let parser = CsvParser::new();
    let outcome = parser
        .parse(&bytes, "quoted_commas_confuse_naive_sniffing.csv", &ParseOptions::new())
        .unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["name", "bio", "age"]);
    assert_eq!(table.rows[0][1], TidyValue::Text("Loves coffee, tea, biscuits".to_string()));
    assert!(outcome.report.notes.iter().any(|n| n.message.contains("delimiter: ';'")));
}

#[test]
fn non_utf8_encoding_is_detected_and_decoded() {
    // Windows-1252 encodes "é" as 0xE9, which is invalid UTF-8 on its own.
    let (encoded, _, _) = encoding_rs::WINDOWS_1252.encode("name;city\nRené;Orléans\n");
    let parser = CsvParser::new();
    let outcome = parser.parse(&encoded, "latin.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];
    assert_eq!(table.rows[0][0], TidyValue::Text("René".to_string()));
    assert!(outcome.report.notes.iter().any(|n| n.message.contains("not valid UTF-8")));
}
