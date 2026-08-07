//! Generates the .avro fixtures committed under fixtures/avro/. Run with
//! `cargo run -p tidyrs-avro --example gen_fixtures_avro` whenever the
//! fixtures need to be regenerated; the resulting files are checked into
//! the repo so tests don't depend on re-running this.

use apache_avro::types::Record;
use apache_avro::{Schema, Writer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/avro");
    std::fs::create_dir_all(&out_dir)?;

    // A mix of primitive types, nullable (union) fields, and a
    // logicalType date — every nullable field is exercised both present
    // and absent across the 5 rows.
    let schema_json = r#"{
        "type": "record",
        "name": "User",
        "fields": [
            {"name": "id", "type": "long"},
            {"name": "name", "type": ["null", "string"], "default": null},
            {"name": "score", "type": ["null", "double"], "default": null},
            {"name": "active", "type": ["null", "boolean"], "default": null},
            {"name": "signup_date", "type": ["null", {"type": "int", "logicalType": "date"}], "default": null}
        ]
    }"#;
    let schema = Schema::parse_str(schema_json)?;

    let mut buffer = Vec::new();
    {
        let mut writer = Writer::new(&schema, &mut buffer);

        type Row = (i64, Option<&'static str>, Option<f64>, Option<bool>, Option<i32>);
        let rows: [Row; 5] = [
            (1, Some("Alice"), Some(91.5), Some(true), Some(0)),       // epoch
            (2, Some("Bob"), Some(-3.25), Some(false), Some(19_723)),  // 2024-01-01
            (3, None, Some(0.0), None, None),                          // several nulls
            (4, Some("Charlotte"), None, Some(true), Some(-1)),        // day before epoch
            (5, Some("大熊"), Some(1e300), Some(false), Some(29_219)), // far future, non-ASCII name
        ];
        for (id, name, score, active, signup_date) in rows {
            let mut record = Record::new(writer.schema()).unwrap();
            record.put("id", id);
            record.put("name", name);
            record.put("score", score);
            record.put("active", active);
            record.put("signup_date", signup_date);
            writer.append(record)?;
        }
        writer.flush()?;
    }

    std::fs::write(out_dir.join("users.avro"), buffer)?;
    println!("wrote fixtures to {}", out_dir.display());
    Ok(())
}
