use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_json::JsonXmlParser;

fn fixture(dir: &str, name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures").join(dir).join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {dir}/{name}: {e}"))
}

fn col<'a>(headers: &[String], row: &'a [TidyValue], name: &str) -> &'a TidyValue {
    let idx = headers
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("no column '{name}' in {headers:?}"));
    &row[idx]
}

#[test]
fn ndjson_is_read_as_one_row_per_line_not_misdetected_as_csv() {
    // Regression (found via external QA testing): an .ndjson file used to
    // get claimed by tidyrs-csv (a comma inside each line's JSON reads as
    // a perfectly consistent CSV "delimiter"), producing garbage — every
    // line split wherever its first comma happened to land, silently, no
    // error anywhere in the pipeline.
    let bytes = fixture("ndjson", "orders.ndjson");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "orders.ndjson", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 3);
    assert_eq!(
        col(&table.headers, &table.rows[0], "customer"),
        &TidyValue::Text("Alice Martin".to_string())
    );
    assert_eq!(col(&table.headers, &table.rows[1], "total"), &TidyValue::Float(17.0));
    assert_eq!(outcome.report.detected_format, "ndjson");
}

#[test]
fn ndjson_is_detected_from_content_alone_over_csv() {
    let bytes = fixture("ndjson", "orders.ndjson");
    let parser = JsonXmlParser::new();
    // No filename hint at all — content-only detection must still win
    // against CSV's own delimiter-consistency scoring.
    assert!(parser.sniff(&bytes, None) > 0.6);
}

#[test]
fn a_key_that_is_sometimes_scalar_array_or_object_does_not_crash_parsing() {
    let bytes = fixture("json", "inconsistent_types.json");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "inconsistent_types.json", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 3);
    assert!(table.headers.contains(&"tags".to_string()));
    assert!(table.headers.contains(&"tags.primary".to_string()));
    assert!(table.headers.contains(&"tags.secondary".to_string()));

    // Alice: scalar tag.
    assert_eq!(col(&table.headers, &table.rows[0], "tags"), &TidyValue::Text("vip".to_string()));
    // Bob: array tag joined into one text value.
    assert_eq!(col(&table.headers, &table.rows[1], "tags"), &TidyValue::Text("new; trial".to_string()));
    // Carla: object tag flattened into sub-columns, "tags" itself absent for her row.
    assert_eq!(col(&table.headers, &table.rows[2], "tags"), &TidyValue::Null);
    assert_eq!(col(&table.headers, &table.rows[2], "tags.primary"), &TidyValue::Text("vip".to_string()));
}

#[test]
fn wrapper_array_under_a_key_is_used_as_the_row_source() {
    let bytes = fixture("json", "wrapped_items.json");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "wrapped_items.json", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 2);
    assert!(table.headers.contains(&"supplier.name".to_string()));
    assert_eq!(col(&table.headers, &table.rows[0], "sku"), &TidyValue::Text("A1".to_string()));
    assert_eq!(
        col(&table.headers, &table.rows[1], "supplier.country"),
        &TidyValue::Text("DE".to_string())
    );
    assert!(outcome
        .report
        .notes
        .iter()
        .any(|n| n.message.contains("used array found under key 'items'")));
}

#[test]
fn single_object_document_becomes_one_row() {
    let bytes = fixture("json", "single_object.json");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "single_object.json", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 1);
    assert_eq!(col(&table.headers, &table.rows[0], "amount"), &TidyValue::Float(120.5));
    assert_eq!(col(&table.headers, &table.rows[0], "paid"), &TidyValue::Bool(true));
}

#[test]
fn default_array_mode_keeps_one_row_per_record_with_indexed_keys() {
    let bytes = fixture("json", "orders_with_line_items.json");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "orders_with_line_items.json", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 2);
    assert!(table.headers.contains(&"items[0].sku".to_string()));
    assert!(table.headers.contains(&"items[1].sku".to_string()));
}

#[test]
fn explode_array_mode_expands_line_items_into_extra_rows() {
    let bytes = fixture("json", "orders_with_line_items.json");
    let parser = JsonXmlParser::new();
    let opts = ParseOptions::new().set("array_mode", "explode");
    let outcome = parser.parse(&bytes, "orders_with_line_items.json", &opts).unwrap();
    let table = &outcome.tables[0];

    // 2 records -> 2 + 1 = 3 rows once items are exploded.
    assert_eq!(table.rows.len(), 3);
    assert!(table.headers.contains(&"items.sku".to_string()));
    assert!(!table.headers.iter().any(|h| h.contains("[0]") || h.contains("[1]")));

    let order_ids: Vec<&TidyValue> = table.rows.iter().map(|r| col(&table.headers, r, "order_id")).collect();
    assert_eq!(order_ids, vec![&TidyValue::Int(1), &TidyValue::Int(1), &TidyValue::Int(2)]);

    let skus: Vec<&TidyValue> = table.rows.iter().map(|r| col(&table.headers, r, "items.sku")).collect();
    assert_eq!(
        skus,
        vec![
            &TidyValue::Text("A1".to_string()),
            &TidyValue::Text("A2".to_string()),
            &TidyValue::Text("B1".to_string())
        ]
    );
    assert!(outcome.report.notes.iter().any(|n| n.message.contains("expanded into")));
}

#[test]
fn yaml_list_of_mappings_becomes_one_row_per_entry() {
    let bytes = fixture("yaml", "list_of_records.yaml");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "list_of_records.yaml", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 3);
    assert_eq!(col(&table.headers, &table.rows[0], "name"), &TidyValue::Text("Alice".to_string()));
    assert_eq!(col(&table.headers, &table.rows[0], "id"), &TidyValue::Int(1));
    assert_eq!(col(&table.headers, &table.rows[0], "active"), &TidyValue::Bool(true));
    assert_eq!(col(&table.headers, &table.rows[1], "active"), &TidyValue::Bool(false));
}

#[test]
fn yaml_single_mapping_document_becomes_one_row() {
    let bytes = fixture("yaml", "single_object.yaml");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "single_object.yaml", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 1);
    assert_eq!(col(&table.headers, &table.rows[0], "amount"), &TidyValue::Float(120.5));
    assert_eq!(col(&table.headers, &table.rows[0], "paid"), &TidyValue::Bool(true));
}

#[test]
fn yaml_wrapper_array_under_a_key_is_used_as_the_row_source() {
    let bytes = fixture("yaml", "wrapped_items.yaml");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "wrapped_items.yaml", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 2);
    assert!(table.headers.contains(&"supplier.name".to_string()));
    assert_eq!(col(&table.headers, &table.rows[0], "sku"), &TidyValue::Text("A1".to_string()));
    assert_eq!(
        col(&table.headers, &table.rows[1], "supplier.country"),
        &TidyValue::Text("DE".to_string())
    );
}

#[test]
fn yaml_key_that_is_sometimes_scalar_list_or_mapping_does_not_crash_parsing() {
    let bytes = fixture("yaml", "inconsistent_types.yaml");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "inconsistent_types.yaml", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 3);
    assert_eq!(col(&table.headers, &table.rows[0], "tags"), &TidyValue::Text("vip".to_string()));
    assert_eq!(col(&table.headers, &table.rows[1], "tags"), &TidyValue::Text("new; trial".to_string()));
    assert_eq!(col(&table.headers, &table.rows[2], "tags.primary"), &TidyValue::Text("vip".to_string()));
}

#[test]
fn yaml_is_detected_from_content_alone_without_a_filename_hint() {
    let bytes = fixture("yaml", "list_of_records.yaml");
    let parser = JsonXmlParser::new();
    assert!(
        parser.sniff(&bytes, None) > 0.5,
        "expected genuine YAML content to score above csv/fixed's ceiling even with no filename hint"
    );
}

#[test]
fn yaml_reports_the_yaml_format_label_not_json() {
    let bytes = fixture("yaml", "single_object.yaml");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "single_object.yaml", &ParseOptions::new()).unwrap();
    assert_eq!(outcome.report.detected_format, "yaml");
}

#[test]
fn xml_repeated_elements_become_rows_with_attributes_flattened() {
    let bytes = fixture("xml", "products.xml");
    let parser = JsonXmlParser::new();
    let outcome = parser.parse(&bytes, "products.xml", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 2);
    assert_eq!(col(&table.headers, &table.rows[0], "@id"), &TidyValue::Text("p1".to_string()));
    assert_eq!(col(&table.headers, &table.rows[0], "price"), &TidyValue::Float(9.99));
    // p2 has an extra <discount> element that p1 doesn't -> missing for p1, filled with Null.
    assert!(table.headers.contains(&"discount".to_string()));
    assert_eq!(col(&table.headers, &table.rows[0], "discount"), &TidyValue::Null);
    assert_eq!(col(&table.headers, &table.rows[1], "discount"), &TidyValue::Float(2.0));
}
