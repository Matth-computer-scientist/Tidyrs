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
fn a_schema_declaring_a_multi_gigabyte_fixed_field_is_rejected_cleanly() {
    // Regression (found via proptest fuzzing, confirmed by reading
    // apache_avro 0.21.0's source directly): a `fixed` schema's `size`
    // comes straight from the file's own embedded schema JSON and is
    // used as `vec![0u8; size]` the instant a value of that type is
    // decoded — the one length in the whole crate that bypasses its own
    // internal `safe_len` allocation guard (every string/bytes/array/map
    // length funnels through it; this one alone doesn't). An adversarial
    // file declaring an implausible size used to crash the whole process
    // outright ("memory allocation of N bytes failed" — an allocator
    // abort, not a panic, so `catch_unwind` cannot save it). This schema
    // never needs to encode a single real value to trigger it: the crash
    // is in `Reader::writer_schema()`-adjacent decoding, not something
    // that depends on `flush()`ing any actual records — see
    // `find_oversized_fixed_type` in src/lib.rs for the fix, which checks
    // the writer schema before the vulnerable decode path is ever reached.
    use apache_avro::{Schema, Writer};

    let schema_json = r#"{
        "type": "record",
        "name": "Malicious",
        "fields": [
            {"name": "payload", "type": {"type": "fixed", "name": "HugeBlob", "size": 21743271952}}
        ]
    }"#;
    let schema = Schema::parse_str(schema_json).unwrap();
    let mut buffer = Vec::new();
    {
        let writer = Writer::new(&schema, &mut buffer);
        // Flushing an empty writer still emits a complete, valid OCF
        // header (magic + metadata map, including the schema above) —
        // exactly what `find_oversized_fixed_type` inspects. No record
        // ever needs to be appended; the vulnerability is in the schema
        // declaration itself, not in decoding an actual 21GB value.
        writer.into_inner().unwrap();
    }

    let parser = AvroParser::new();
    let result = parser.parse(&buffer, "malicious.avro", &ParseOptions::new());

    let message = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("a file declaring an implausible fixed-field size must be rejected, not accepted or crash the process"),
    };
    assert!(
        message.contains("fixed") && message.contains("21743271952"),
        "expected the error to name the offending field, got: {message}"
    );
}

#[test]
fn a_header_declaring_an_implausible_metadata_entry_count_is_rejected_cleanly() {
    // Regression: the exact input the fuzz suite actually found (captured
    // directly from a crashing proptest run, not reconstructed from
    // theory). Just the magic bytes plus a handful of adversarial bytes —
    // no valid schema JSON, no real data — is enough: the OCF header's
    // own metadata is Avro-encoded as `map<bytes>`, and decoding it reads
    // a declared entry count that apache_avro's `safe_len` validates as
    // if it were a *byte length* (comfortably under its 512MiB default)
    // but is then used directly as an *element count* for
    // `HashMap::reserve` — where each `(String, Value)` entry is closer
    // to 100+ bytes. A "safely small" count multiplies out to a real
    // ~21.7GB allocation attempt, aborting the process before
    // `catch_unwind` (which only catches panics, not allocator failures)
    // ever gets a chance. See `header_metadata_count_is_plausible` in
    // src/lib.rs for the fix and the module docs for the full mechanism.
    let bytes: Vec<u8> = vec![79, 98, 106, 1, 165, 238, 164, 126, 123, 109, 61, 2];
    let parser = AvroParser::new();
    let result = parser.parse(&bytes, "adversarial.avro", &ParseOptions::new());

    let message = match result {
        Err(e) => e.to_string(),
        Ok(_) => panic!("a header declaring an implausible metadata entry count must be rejected, not accepted or crash the process"),
    };
    assert!(
        message.contains("metadata"),
        "expected the error to describe the implausible header, got: {message}"
    );
}

#[test]
fn sniff_recognizes_avro_files_from_content_alone() {
    let bytes = fixture("users.avro");
    let parser = AvroParser::new();
    assert!(parser.sniff(&bytes, None) > 0.5);
    assert!(parser.sniff(&bytes, Some("users.avro")) > 0.8);
}
