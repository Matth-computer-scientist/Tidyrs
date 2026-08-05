use tidyrs_core::{validate, Schema, TidyTable, TidyValue};

fn sample_table() -> TidyTable {
    let mut t = TidyTable::new(vec!["id".into(), "name".into(), "amount".into(), "active".into()]);
    t.push_row(vec![
        TidyValue::Int(1),
        TidyValue::Text("Alice".into()),
        TidyValue::Float(9.99),
        TidyValue::Bool(true),
    ]);
    t.push_row(vec![
        TidyValue::Int(2),
        TidyValue::Text("Bob".into()),
        TidyValue::Null,
        TidyValue::Bool(false),
    ]);
    t.push_row(vec![
        TidyValue::Int(3),
        TidyValue::Text("Carla".into()),
        TidyValue::Text("oops".into()),
        TidyValue::Bool(true),
    ]);
    t
}

fn schema_json() -> &'static str {
    r#"{
        "columns": [
            {"name": "id", "type": "integer", "nullable": false},
            {"name": "name", "type": "text"},
            {"name": "amount", "type": "float"},
            {"name": "active", "type": "boolean"}
        ]
    }"#
}

#[test]
fn valid_columns_produce_no_issues() {
    let table = sample_table();
    let schema = Schema::from_json(schema_json()).unwrap();
    let report = validate(&table, &schema);

    // row 2's "amount" is Text("oops") where Float is expected -> 1 issue.
    // row 1's "amount" being Null is fine since nullable defaults true.
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].column, "amount");
    assert_eq!(report.issues[0].row, Some(2));
    assert!(!report.is_valid());
}

#[test]
fn non_nullable_column_flags_null_values() {
    let mut table = sample_table();
    table.rows[1][0] = TidyValue::Null; // id is declared non-nullable
    let schema = Schema::from_json(schema_json()).unwrap();
    let report = validate(&table, &schema);

    assert!(report
        .issues
        .iter()
        .any(|i| i.column == "id" && i.row == Some(1) && i.message.contains("non-nullable")));
}

#[test]
fn missing_declared_column_is_a_table_level_issue() {
    let table = TidyTable::new(vec!["id".into()]);
    let schema = Schema::from_json(schema_json()).unwrap();
    let report = validate(&table, &schema);

    let missing: Vec<&str> = report.issues.iter().filter(|i| i.row.is_none()).map(|i| i.column.as_str()).collect();
    assert!(missing.contains(&"name"));
    assert!(missing.contains(&"amount"));
    assert!(missing.contains(&"active"));
}

#[test]
fn strict_mode_flags_undeclared_columns() {
    let table = sample_table();
    let mut schema = Schema::from_json(schema_json()).unwrap();
    schema.strict = true;
    schema.columns.retain(|c| c.name != "active"); // leave "active" undeclared

    let report = validate(&table, &schema);
    assert!(report
        .issues
        .iter()
        .any(|i| i.row.is_none() && i.column == "active" && i.message.contains("strict mode")));
}

#[test]
fn integer_column_rejects_float_values() {
    let mut table = TidyTable::new(vec!["qty".into()]);
    table.push_row(vec![TidyValue::Float(1.5)]);
    let schema = Schema::from_json(r#"{"columns":[{"name":"qty","type":"integer"}]}"#).unwrap();

    let report = validate(&table, &schema);
    assert_eq!(report.issues.len(), 1);
}

#[test]
fn float_column_accepts_integers_too() {
    let mut table = TidyTable::new(vec!["qty".into()]);
    table.push_row(vec![TidyValue::Int(5)]);
    let schema = Schema::from_json(r#"{"columns":[{"name":"qty","type":"float"}]}"#).unwrap();

    let report = validate(&table, &schema);
    assert!(report.is_valid());
}
