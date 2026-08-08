use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_csv::CsvParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/csv").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name}: {e}"))
}

#[test]
fn a_repeated_header_name_is_disambiguated_not_left_ambiguous() {
    // Regression (found via manual QA testing): a source file's own
    // header row repeating a name (a duplicate "id" column, a real
    // spreadsheet-to-CSV export artifact) used to be passed straight
    // through unchanged, leaving two output columns with the exact same
    // name that anything indexing by column name couldn't tell apart.
    // tidyrs-xlsx already disambiguates its own header row this way.
    let bytes = b"id,id,name\n1,2,Bob\n".to_vec();
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "dup.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["id", "id_2", "name"]);
}

#[test]
fn rows_in_excludes_the_header_row() {
    // Regression: rows_in used to count the header as a data row, so a
    // file with 1 header + 3 data rows reported rows_in=4 while
    // rows_out=3 for the exact same rows (none dropped) — internally
    // inconsistent within a single report, and different from the
    // --stream path's (correct) count. See tidyrs-csv/src/stream.rs for
    // the streaming side of this invariant.
    let bytes = b"name,age\nAlice,30\nBob,41\nCharlotte,25\n".to_vec();
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "in.csv", &ParseOptions::new()).unwrap();

    assert_eq!(outcome.report.rows_in, 3);
    assert_eq!(outcome.report.rows_out, 3);
    assert_eq!(outcome.report.rows_in, outcome.report.rows_out);
}

#[test]
fn in_memory_and_streaming_report_the_same_row_counts() {
    let bytes = b"name;age;city\nAlice;30;Paris\nBob;;Lyon\nCharlotte;25;Marseille\n".to_vec();

    let parser = CsvParser::new();
    let in_memory = parser.parse(&bytes, "in.csv", &ParseOptions::new()).unwrap();

    let mut streamed_out = Vec::new();
    let streamed_report = tidyrs_csv::stream_clean_csv(bytes.as_slice(), &mut streamed_out, "in.csv", &ParseOptions::new()).unwrap();

    assert_eq!(in_memory.report.rows_in, streamed_report.rows_in);
    assert_eq!(in_memory.report.rows_out, streamed_report.rows_out);
    assert_eq!(in_memory.report.rows_in, 3);
    assert_eq!(in_memory.report.rows_out, 3);
}

#[test]
fn sniff_rejects_content_that_is_mostly_control_characters() {
    // Regression: bytes that decode (under a guessed encoding) to mostly
    // control characters used to occasionally score high enough to be
    // misdetected as CSV, purely because a stray 0x0A/delimiter-like byte
    // showed up by chance. Build a deterministic "looks binary" buffer
    // (repeating low control bytes with a couple of real newlines mixed
    // in, so it has >=2 non-empty "lines" like real binary garbage would)
    // rather than relying on true randomness, which would be flaky in CI.
    let mut junk = vec![0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
    junk.extend([b'\n']);
    junk.extend([0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    junk.extend([b'\n']);
    junk.extend(vec![0x01u8; 100]);

    let parser = CsvParser::new();
    assert_eq!(parser.sniff(&junk, Some("mystery.csv")), 0.0);
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

#[test]
fn a_leading_utf8_bom_does_not_pollute_the_first_header_name() {
    // Regression (found via external QA testing): a BOM is valid UTF-8
    // (decodes to U+FEFF), so it survived str::from_utf8 unchanged and
    // glued itself onto the first header's name — "\u{FEFF}id" instead of
    // "id" — breaking any downstream lookup by that column's real name.
    // Real-world source: Excel's own "CSV UTF-8" save option always
    // writes one.
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"id,name\n1,Alice\n");
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "bom.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["id", "name"]);
    assert_eq!(table.rows[0][0], TidyValue::Int(1));
}

#[test]
fn a_postal_code_with_a_leading_zero_is_not_silently_converted_to_a_number() {
    // Regression (found via external QA testing): "00501" used to become
    // 501 with no warning — real, silent data corruption on identifiers
    // that are never supposed to be treated as arithmetic numbers.
    let bytes = b"zip,city\n00501,Holtsville\n02134,Boston\n06390,Fishers Island\n00544,Holtsville\n".to_vec();
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "zips.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    for (i, expected) in ["00501", "02134", "06390", "00544"].iter().enumerate() {
        assert_eq!(
            table.rows[i][0],
            TidyValue::Text(expected.to_string()),
            "leading zero must survive in row {i}: {:?}",
            table.rows[i]
        );
    }
}

#[test]
fn an_integer_too_big_for_i64_is_not_silently_rounded_through_a_float() {
    // Regression (found via external QA testing, round 4): a whole
    // number too big for i64 used to fall through to str::parse::<f64>,
    // which only has ~15-17 significant decimal digits of precision —
    // "9999999999999999999" (20 digits) silently became
    // "10000000000000000000", and i64::MIN silently became
    // "-9223372036854776000" whenever it landed in a column resolved to
    // Float (e.g. alongside other overflowing values). Both must keep
    // their exact digits as text instead.
    let bytes = b"id,amount\n1,9999999999999999999\n2,-9223372036854775808\n".to_vec();
    let parser = CsvParser::new();
    let outcome = parser.parse(&bytes, "overflow.csv", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows[0][1], TidyValue::Text("9999999999999999999".to_string()));
    assert_eq!(table.rows[1][1], TidyValue::Text("-9223372036854775808".to_string()));
}
