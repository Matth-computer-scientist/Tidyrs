//! Generates the .parquet fixtures committed under fixtures/parquet/. Run
//! with `cargo run -p tidyrs-parquet --example gen_fixtures_parquet` whenever the
//! fixtures need to be regenerated; the resulting files are checked into
//! the repo so tests don't depend on re-running this.

use arrow_array::{BooleanArray, Date32Array, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/parquet");
    std::fs::create_dir_all(&out_dir)?;

    // A mix of primitive types plus nulls in every column, the same
    // "cover the boring-but-common case thoroughly" spirit as the other
    // crates' fixtures.
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, true),
        Field::new("score", DataType::Float64, true),
        Field::new("active", DataType::Boolean, true),
        Field::new("signup_date", DataType::Date32, true),
    ]));

    let ids = Arc::new(Int64Array::from(vec![1, 2, 3, 4, 5]));
    let names = Arc::new(StringArray::from(vec![Some("Alice"), Some("Bob"), None, Some("Charlotte"), Some("大熊")]));
    let scores = Arc::new(Float64Array::from(vec![
        Some(91.5),
        Some(-3.25),
        Some(0.0),
        None,
        Some(f64::MAX.min(1e300)),
    ]));
    let active = Arc::new(BooleanArray::from(vec![Some(true), Some(false), None, Some(true), Some(false)]));
    // Date32 is days since the epoch; 0 = 1970-01-01.
    let signup_dates = Arc::new(Date32Array::from(vec![Some(0), Some(19_723), None, Some(-1), Some(29_219)]));

    let batch = RecordBatch::try_new(schema.clone(), vec![ids, names, scores, active, signup_dates])?;

    let file = std::fs::File::create(out_dir.join("users.parquet"))?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;

    println!("wrote fixtures to {}", out_dir.display());
    Ok(())
}
