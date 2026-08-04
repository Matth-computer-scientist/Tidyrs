use parquet::file::reader::{FileReader, SerializedFileReader};
use tidyrs_core::{export, TidyTable, TidyValue};

#[test]
fn columns_get_a_narrow_type_instead_of_all_strings() {
    let mut table = TidyTable::new(vec!["id".into(), "score".into(), "active".into(), "label".into()]);
    table.push_row(vec![TidyValue::Int(1), TidyValue::Int(10), TidyValue::Bool(true), TidyValue::Text("a".into())]);
    table.push_row(vec![TidyValue::Int(2), TidyValue::Float(2.5), TidyValue::Bool(false), TidyValue::Int(42)]);
    table.push_row(vec![TidyValue::Int(3), TidyValue::Null, TidyValue::Bool(true), TidyValue::Text("c".into())]);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.parquet");
    export::write_parquet_file(&table, &path).unwrap();

    let file = std::fs::File::open(&path).unwrap();
    let reader = SerializedFileReader::new(file).unwrap();
    let schema = reader.metadata().file_metadata().schema();
    let fields = schema.get_fields();

    // id: all Int -> INT64
    assert_eq!(fields[0].get_physical_type().to_string(), "INT64");
    // score: Int + Float mixed -> DOUBLE (ints promoted, not truncated)
    assert_eq!(fields[1].get_physical_type().to_string(), "DOUBLE");
    // active: all Bool -> BOOLEAN
    assert_eq!(fields[2].get_physical_type().to_string(), "BOOLEAN");
    // label: Text + Int mixed -> falls back to BYTE_ARRAY (Utf8) rather
    // than coercing or dropping the outlier value.
    assert_eq!(fields[3].get_physical_type().to_string(), "BYTE_ARRAY");

    assert_eq!(reader.metadata().file_metadata().num_rows(), 3);
}
