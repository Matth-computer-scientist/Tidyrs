use tidyrs_avro::AvroParser;
use tidyrs_core::{ParseOptions, TidyParser, TidyValue};

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/avro").join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {name} (run `cargo run -p tidyrs-avro --example gen_fixtures_avro`): {e}"))
}

fn col<'a>(headers: &[String], row: &'a [TidyValue], name: &str) -> &'a TidyValue {
    let idx = headers
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("no column '{name}' in {headers:?}"));
    &row[idx]
}

#[test]
fn primitive_types_and_nullable_union_fields_are_read_with_correct_native_typing() {
    let bytes = fixture("users.avro");
    let parser = AvroParser::new();
    let outcome = parser.parse(&bytes, "users.avro", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["id", "name", "score", "active", "signup_date"]);
    assert_eq!(table.rows.len(), 5);

    // Int/Float/Boolean map to native TidyValue variants, not Text — and
    // every one of these fields is a `["null", T]` union in the schema,
    // proving Union unwrapping (not just plain scalars) produces native
    // types.
    assert_eq!(col(&table.headers, &table.rows[0], "id"), &TidyValue::Int(1));
    assert_eq!(col(&table.headers, &table.rows[0], "score"), &TidyValue::Float(91.5));
    assert_eq!(col(&table.headers, &table.rows[1], "active"), &TidyValue::Bool(false));

    // A null field comes through as TidyValue::Null (unwrapped from
    // Union(0, Null)), not some literal "null" text or a crash.
    assert_eq!(col(&table.headers, &table.rows[2], "name"), &TidyValue::Null);
    assert_eq!(col(&table.headers, &table.rows[2], "active"), &TidyValue::Null);
    assert_eq!(col(&table.headers, &table.rows[2], "signup_date"), &TidyValue::Null);
    assert_eq!(col(&table.headers, &table.rows[3], "score"), &TidyValue::Null);

    // The logicalType "date" field resolves to a real calendar date (the
    // documented conversion — see module docs): day 0 is the epoch, and a
    // negative day count must resolve to the day *before* the epoch, not
    // panic or produce garbage.
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
fn a_nested_record_field_renders_as_readable_text_instead_of_erroring() {
    use apache_avro::types::{Record, Value};
    use apache_avro::{Schema, Writer};

    let schema_json = r#"{
        "type": "record",
        "name": "Order",
        "fields": [
            {"name": "order_id", "type": "long"},
            {"name": "customer", "type": {
                "type": "record",
                "name": "Customer",
                "fields": [
                    {"name": "name", "type": "string"},
                    {"name": "country", "type": "string"}
                ]
            }}
        ]
    }"#;
    let schema = Schema::parse_str(schema_json).unwrap();
    let mut buffer = Vec::new();
    {
        let mut writer = Writer::new(&schema, &mut buffer);
        let mut record = Record::new(writer.schema()).unwrap();
        record.put("order_id", 42i64);
        record.put(
            "customer",
            Value::Record(vec![
                ("name".to_string(), Value::String("Alice".to_string())),
                ("country".to_string(), Value::String("FR".to_string())),
            ]),
        );
        writer.append(record).unwrap();
        writer.flush().unwrap();
    }

    let parser = AvroParser::new();
    let outcome = parser.parse(&buffer, "orders.avro", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.headers, vec!["order_id", "customer"]);
    match col(&table.headers, &table.rows[0], "customer") {
        TidyValue::Text(s) => {
            assert!(
                s.contains("Alice"),
                "expected the nested record's fields to appear in the text, got {s:?}"
            );
            assert!(s.contains("FR"), "expected the nested record's fields to appear in the text, got {s:?}");
        }
        other => panic!("expected a Text value for the nested record column, got {other:?}"),
    }
}

#[test]
fn sniff_recognizes_avro_files_from_content_alone() {
    let bytes = fixture("users.avro");
    let parser = AvroParser::new();
    assert!(parser.sniff(&bytes, None) > 0.5);
    assert!(parser.sniff(&bytes, Some("users.avro")) > 0.8);
}
