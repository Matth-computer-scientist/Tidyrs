use crate::error::{TidyError, TidyResult};
use crate::table::TidyTable;
use crate::value::TidyValue;
use std::io::Write;
use std::sync::Arc;

pub fn write_csv<W: Write>(table: &TidyTable, writer: W) -> TidyResult<()> {
    let mut wtr = csv::Writer::from_writer(writer);
    wtr.write_record(&table.headers)?;
    for row in &table.rows {
        let record: Vec<String> = row.iter().map(|v| v.as_export_string()).collect();
        wtr.write_record(&record)?;
    }
    wtr.flush()?;
    Ok(())
}

pub fn write_json<W: Write>(table: &TidyTable, writer: W) -> TidyResult<()> {
    let mut objects = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let mut obj = serde_json::Map::new();
        for (header, value) in table.headers.iter().zip(row.iter()) {
            obj.insert(header.clone(), serde_json::to_value(value)?);
        }
        objects.push(serde_json::Value::Object(obj));
    }
    serde_json::to_writer_pretty(writer, &serde_json::Value::Array(objects))?;
    Ok(())
}

/// The physical Parquet type chosen for one column, inferred from its
/// actual cell values rather than fixed to string for every column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnPlan {
    Int64,
    Double,
    Boolean,
    Utf8,
}

/// Inspects every value in a column and picks the narrowest type that
/// fits all of them: all-Int -> Int64, all-Int/Float -> Double (ints are
/// promoted, never truncated), all-Bool -> Boolean, anything else (mixed
/// types, or text) -> Utf8. An all-null column defaults to Utf8. This is
/// intentionally conservative: a single value that doesn't fit the
/// majority's type falls the whole column back to Utf8 rather than
/// silently dropping or coercing it.
fn infer_column_plan(cells: &[TidyValue]) -> ColumnPlan {
    let non_null: Vec<&TidyValue> = cells.iter().filter(|v| !v.is_null()).collect();
    if non_null.is_empty() {
        return ColumnPlan::Utf8;
    }
    if non_null.iter().all(|v| matches!(v, TidyValue::Int(_))) {
        return ColumnPlan::Int64;
    }
    if non_null.iter().all(|v| matches!(v, TidyValue::Int(_) | TidyValue::Float(_))) {
        return ColumnPlan::Double;
    }
    if non_null.iter().all(|v| matches!(v, TidyValue::Bool(_))) {
        return ColumnPlan::Boolean;
    }
    ColumnPlan::Utf8
}

fn column_cells(table: &TidyTable, col_idx: usize) -> Vec<TidyValue> {
    table.rows.iter().map(|row| row.get(col_idx).cloned().unwrap_or(TidyValue::Null)).collect()
}

/// Writes a table as Parquet, inferring one of Int64/Double/Boolean/Utf8
/// per column from its actual values (see [`infer_column_plan`]) instead
/// of exporting everything as strings. A column falls back to Utf8 the
/// moment any single value doesn't fit the narrower type shared by the
/// rest — mixed-type columns are common in chaotic input, and silently
/// coercing or dropping the outlier value would defeat the point of an
/// auditable cleaning tool.
pub fn write_parquet_file(table: &TidyTable, path: &std::path::Path) -> TidyResult<()> {
    use parquet::basic::{Compression, ConvertedType, Repetition, Type as PhysicalType};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::types::Type as SchemaType;

    let plans: Vec<ColumnPlan> = (0..table.headers.len())
        .map(|i| infer_column_plan(&column_cells(table, i)))
        .collect();

    let fields: Vec<Arc<SchemaType>> = table
        .headers
        .iter()
        .zip(plans.iter())
        .map(|(h, plan)| {
            let mut builder = match plan {
                ColumnPlan::Int64 => SchemaType::primitive_type_builder(h, PhysicalType::INT64),
                ColumnPlan::Double => SchemaType::primitive_type_builder(h, PhysicalType::DOUBLE),
                ColumnPlan::Boolean => SchemaType::primitive_type_builder(h, PhysicalType::BOOLEAN),
                ColumnPlan::Utf8 => {
                    SchemaType::primitive_type_builder(h, PhysicalType::BYTE_ARRAY).with_converted_type(ConvertedType::UTF8)
                }
            };
            builder = builder.with_repetition(Repetition::OPTIONAL);
            Arc::new(builder.build().expect("valid parquet primitive type"))
        })
        .collect();

    let schema = Arc::new(
        SchemaType::group_type_builder("tidyloom_table")
            .with_fields(fields)
            .build()
            .map_err(|e| TidyError::Export(e.to_string()))?,
    );

    let props = Arc::new(WriterProperties::builder().set_compression(Compression::SNAPPY).build());
    let file = std::fs::File::create(path)?;
    let mut writer = SerializedFileWriter::new(file, schema, props).map_err(|e| TidyError::Export(e.to_string()))?;
    let mut row_group_writer = writer.next_row_group().map_err(|e| TidyError::Export(e.to_string()))?;

    let mut col_idx = 0;
    while let Some(mut col_writer) = row_group_writer.next_column().map_err(|e| TidyError::Export(e.to_string()))? {
        use parquet::column::writer::ColumnWriter;
        use parquet::data_type::ByteArray;

        let cells = column_cells(table, col_idx);
        let def_levels: Vec<i16> = cells.iter().map(|v| if v.is_null() { 0 } else { 1 }).collect();

        match (plans[col_idx], col_writer.untyped()) {
            (ColumnPlan::Int64, ColumnWriter::Int64ColumnWriter(ref mut w)) => {
                let values: Vec<i64> = cells
                    .iter()
                    .filter_map(|v| match v {
                        TidyValue::Int(i) => Some(*i),
                        _ => None,
                    })
                    .collect();
                w.write_batch(&values, Some(&def_levels), None).map_err(|e| TidyError::Export(e.to_string()))?;
            }
            (ColumnPlan::Double, ColumnWriter::DoubleColumnWriter(ref mut w)) => {
                let values: Vec<f64> = cells
                    .iter()
                    .filter_map(|v| match v {
                        TidyValue::Int(i) => Some(*i as f64),
                        TidyValue::Float(f) => Some(*f),
                        _ => None,
                    })
                    .collect();
                w.write_batch(&values, Some(&def_levels), None).map_err(|e| TidyError::Export(e.to_string()))?;
            }
            (ColumnPlan::Boolean, ColumnWriter::BoolColumnWriter(ref mut w)) => {
                let values: Vec<bool> = cells
                    .iter()
                    .filter_map(|v| match v {
                        TidyValue::Bool(b) => Some(*b),
                        _ => None,
                    })
                    .collect();
                w.write_batch(&values, Some(&def_levels), None).map_err(|e| TidyError::Export(e.to_string()))?;
            }
            (ColumnPlan::Utf8, ColumnWriter::ByteArrayColumnWriter(ref mut w)) => {
                let values: Vec<ByteArray> = cells
                    .iter()
                    .filter(|v| !v.is_null())
                    .map(|v| ByteArray::from(v.as_export_string().as_bytes().to_vec()))
                    .collect();
                w.write_batch(&values, Some(&def_levels), None).map_err(|e| TidyError::Export(e.to_string()))?;
            }
            _ => unreachable!("column writer physical type always matches the schema built from the same plan"),
        }

        col_writer.close().map_err(|e| TidyError::Export(e.to_string()))?;
        col_idx += 1;
    }

    row_group_writer.close().map_err(|e| TidyError::Export(e.to_string()))?;
    writer.close().map_err(|e| TidyError::Export(e.to_string()))?;
    Ok(())
}
