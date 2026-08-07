use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_orc::OrcParser;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/orc").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name}: {e}"))
}

fn col<'a>(headers: &[String], row: &'a [TidyValue], name: &str) -> &'a TidyValue {
    let idx = headers
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("no column '{name}' in {headers:?}"));
    &row[idx]
}

// alltypes.snappy.orc is `orc-rust`'s own (Apache-2.0) test fixture,
// covering every ORC primitive type plus Snappy compression, nulls, and
// the extremes of each integer/float width — reused here rather than
// hand-built so the expected values are independently verified against
// upstream's own test suite (tests/basic/main.rs::alltypes_test), not
// just "whatever this crate happens to produce."
#[test]
#[allow(clippy::approx_constant)] // the fixture's actual value, not a mistaken pi literal
fn all_primitive_types_snappy_compressed_are_read_with_correct_native_typing() {
    let bytes = fixture("alltypes.snappy.orc");
    let parser = OrcParser::new();
    let outcome = parser.parse(&bytes, "alltypes.snappy.orc", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(
        table.headers,
        vec!["boolean", "int8", "int16", "int32", "int64", "float32", "float64", "decimal", "binary", "utf8", "date32"]
    );
    // 11 rows: a leading all-null row, 9 data rows, a trailing all-null row.
    assert_eq!(table.rows.len(), 11);

    // Leading and trailing rows are entirely null.
    assert!(table.rows[0].iter().all(|v| v == &TidyValue::Null));
    assert!(table.rows[10].iter().all(|v| v == &TidyValue::Null));

    // Boolean/Int/Float map to their native TidyValue variants, not Text.
    assert_eq!(col(&table.headers, &table.rows[1], "boolean"), &TidyValue::Bool(true));
    assert_eq!(col(&table.headers, &table.rows[4], "int8"), &TidyValue::Int(127));
    assert_eq!(col(&table.headers, &table.rows[5], "int8"), &TidyValue::Int(-128));
    assert_eq!(col(&table.headers, &table.rows[4], "int64"), &TidyValue::Int(i64::MAX));
    assert_eq!(col(&table.headers, &table.rows[5], "int64"), &TidyValue::Int(i64::MIN));
    assert_eq!(col(&table.headers, &table.rows[6], "float64"), &TidyValue::Float(3.14159265359));
    assert_eq!(col(&table.headers, &table.rows[4], "float32"), &TidyValue::Float(f64::INFINITY));

    // Decimal/date/binary/utf8 render as text (the documented
    // simplification — see the module docs on why these aren't
    // hand-converted to a native variant).
    assert_eq!(col(&table.headers, &table.rows[1], "decimal"), &TidyValue::Text("0.00000".to_string()));
    assert_eq!(col(&table.headers, &table.rows[1], "date32"), &TidyValue::Text("1970-01-01".to_string()));
    // Non-ASCII text (Japanese, an emoji) must survive intact.
    assert_eq!(col(&table.headers, &table.rows[6], "utf8"), &TidyValue::Text("大熊和奏".to_string()));
    assert_eq!(col(&table.headers, &table.rows[9], "utf8"), &TidyValue::Text("🤔".to_string()));

    assert_eq!(outcome.report.rows_in, 11);
    assert_eq!(outcome.report.rows_out, 11);
}

// nested_struct.orc: a Struct<a: double, b: boolean> column — proves the
// documented "nested types render as Arrow's own display text, not
// flattened sub-columns" behavior actually produces something sane rather
// than erroring out or panicking.
#[test]
fn a_nested_struct_column_renders_as_readable_text_instead_of_erroring() {
    let bytes = fixture("nested_struct.orc");
    let parser = OrcParser::new();
    let outcome = parser.parse(&bytes, "nested_struct.orc", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 5);
    match &table.rows[0][0] {
        TidyValue::Text(s) => {
            assert!(s.contains('1'), "expected the struct's numeric field to appear in the text, got {s:?}");
            assert!(s.contains("true"), "expected the struct's boolean field to appear in the text, got {s:?}");
        }
        other => panic!("expected a Text value for the nested struct column, got {other:?}"),
    }
    // A row where the whole struct is null must still come through as
    // TidyValue::Null, not e.g. the literal text "null".
    assert_eq!(table.rows[3][0], TidyValue::Null);
}

#[test]
fn sniff_recognizes_orc_files_from_content_alone() {
    let bytes = fixture("alltypes.snappy.orc");
    let parser = OrcParser::new();
    assert!(parser.sniff(&bytes, None) > 0.5);
    assert!(parser.sniff(&bytes, Some("alltypes.snappy.orc")) > 0.8);
}
