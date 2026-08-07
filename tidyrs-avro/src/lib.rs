//! Apache Avro reading for tidyloom — **experimental**.
//!
//! Reads Avro Object Container Files (the `.avro` files real pipelines
//! actually pass around — a self-describing container embedding its own
//! writer schema, as opposed to bare Avro-encoded bytes needing an
//! external schema). One writer schema applies to the *whole* file
//! (unlike JSON, where each record can independently vary in shape), so
//! the header list comes straight from the first record's field order
//! rather than a union-of-all-records scan.
//!
//! `apache_avro::types::Value` is considerably richer than `TidyValue`.
//! `Union` (Avro's usual `["null", T]` encoding for an optional field) is
//! transparently unwrapped rather than surfaced as-is, since otherwise
//! *every* nullable field in a real-world schema would show up as an
//! opaque wrapper instead of its actual value. Logical date/time types
//! are converted to real calendar dates/timestamps via `chrono`. Nested
//! `Record`/`Array`/`Map` values, and `Decimal`/`Duration`/`Fixed`
//! (types with no natural short text form), fall back to Rust's `Debug`
//! formatting — readable and lossless, but not flattened into
//! sub-columns the way `tidyrs-json`'s JSON/YAML flattening is, and not
//! independently round-trippable the way a hand-written decimal-to-
//! string conversion would be. This mirrors the same "native type where
//! unambiguous, readable text where not" policy `tidyrs-orc` and
//! `tidyrs-parquet` already use for their own rich schemas.

use apache_avro::types::Value;
use apache_avro::Reader;
use tidyrs_core::{
    AmbiguityResolver, CleaningReport, ParseOptions, ParseOutcome, RuleBasedResolver, TidyError, TidyParser, TidyResult, TidyTable, TidyValue,
};

pub struct AvroParser {
    resolver: Box<dyn AmbiguityResolver>,
}

impl AvroParser {
    pub fn new() -> Self {
        Self {
            resolver: Box::new(RuleBasedResolver),
        }
    }

    /// See `tidyrs_csv::CsvParser::with_resolver` — same idea, same
    /// extension point.
    pub fn with_resolver(resolver: Box<dyn AmbiguityResolver>) -> Self {
        Self { resolver }
    }
}

impl Default for AvroParser {
    fn default() -> Self {
        Self::new()
    }
}

/// The fixed 4-byte Object Container File magic: "Obj" followed by the
/// format version byte (currently always 1).
const MAGIC: &[u8] = b"Obj\x01";

fn has_avro_extension(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".avro")
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn date_from_epoch_days(days: i32) -> String {
    match chrono::NaiveDate::from_ymd_opt(1970, 1, 1).and_then(|epoch| epoch.checked_add_signed(chrono::Duration::days(days as i64))) {
        Some(d) => d.format("%Y-%m-%d").to_string(),
        None => format!("<date out of range: {days} days from epoch>"),
    }
}

fn timestamp_from_millis(ms: i64) -> String {
    match chrono::DateTime::from_timestamp_millis(ms) {
        Some(t) => t.to_rfc3339(),
        None => format!("<timestamp out of range: {ms}ms>"),
    }
}

fn timestamp_from_micros(us: i64) -> String {
    match chrono::DateTime::from_timestamp_micros(us) {
        Some(t) => t.to_rfc3339(),
        None => format!("<timestamp out of range: {us}us>"),
    }
}

/// Converts one Avro `Value` into a `TidyValue`. `Union` is unwrapped
/// recursively (see module docs); everything without a clean scalar
/// mapping falls back to `Debug` text.
fn avro_value_to_tidy(value: &Value) -> TidyValue {
    match value {
        Value::Null => TidyValue::Null,
        Value::Boolean(b) => TidyValue::Bool(*b),
        Value::Int(i) => TidyValue::Int(*i as i64),
        Value::Long(i) => TidyValue::Int(*i),
        Value::Float(f) => TidyValue::Float(*f as f64),
        Value::Double(f) => TidyValue::Float(*f),
        Value::String(s) => TidyValue::Text(s.clone()),
        Value::Bytes(b) => TidyValue::Text(hex_encode(b)),
        Value::Fixed(_, b) => TidyValue::Text(hex_encode(b)),
        Value::Enum(_, symbol) => TidyValue::Text(symbol.clone()),
        Value::Union(_, boxed) => avro_value_to_tidy(boxed),
        Value::Date(days) => TidyValue::Text(date_from_epoch_days(*days)),
        Value::TimeMillis(ms) => TidyValue::Text(format!("{ms}ms since midnight")),
        Value::TimeMicros(us) => TidyValue::Text(format!("{us}us since midnight")),
        Value::TimestampMillis(ms) => TidyValue::Text(timestamp_from_millis(*ms)),
        Value::TimestampMicros(us) => TidyValue::Text(timestamp_from_micros(*us)),
        Value::Uuid(u) => TidyValue::Text(u.to_string()),
        // Decimal/Duration/nested Record/Array/Map — no clean short text
        // form; Debug is readable and complete, if not itself
        // machine-parseable. See module docs.
        other => TidyValue::Text(format!("{other:?}")),
    }
}

impl TidyParser for AvroParser {
    fn format_name(&self) -> &'static str {
        "avro"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let mut score: f32 = 0.0;
        if let Some(name) = filename {
            if has_avro_extension(name) {
                score += 0.3;
            }
        }
        if bytes.starts_with(MAGIC) {
            score += 0.6;
        }
        score.min(1.0)
    }

    fn parse(&self, bytes: &[u8], filename: &str, _options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let mut report = CleaningReport::new(filename, self.format_name());
        report.warning(
            "Avro support is experimental in this version: nested Record/Array/Map values and Decimal/Duration \
             fields are rendered as Rust debug text rather than a fully typed conversion or flattening — \
             see the tidyrs-avro module docs for exactly what this does and doesn't handle."
                .to_string(),
        );

        if !bytes.starts_with(MAGIC) {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "not an Avro Object Container File (missing the 'Obj\\x01' magic header)".into(),
            });
        }

        // apache-avro's decoder isn't proptest-hardened in this workspace
        // the way calamine/pdf-extract/rusqlite have been; isolate it
        // behind catch_unwind on principle, consistent with every other
        // parser here that wraps a third-party binary-format decoder.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let reader = Reader::new(bytes).map_err(|e| e.to_string())?;
            let mut headers: Option<Vec<String>> = None;
            let mut raw_rows: Vec<Vec<TidyValue>> = Vec::new();
            for value in reader {
                let value = value.map_err(|e| e.to_string())?;
                match value {
                    Value::Record(fields) => {
                        if headers.is_none() {
                            headers = Some(fields.iter().map(|(k, _)| k.clone()).collect());
                        }
                        raw_rows.push(fields.iter().map(|(_, v)| avro_value_to_tidy(v)).collect());
                    }
                    // A file whose top-level writer schema isn't a record
                    // (legal in Avro, e.g. a plain array/string/int
                    // schema) — treat the whole value as one unnamed
                    // column, the same "whole document as a single
                    // record" fallback tidyrs-json uses for a bare JSON
                    // scalar.
                    other => {
                        if headers.is_none() {
                            headers = Some(vec!["value".to_string()]);
                        }
                        raw_rows.push(vec![avro_value_to_tidy(&other)]);
                    }
                }
            }
            Ok::<_, String>((headers.unwrap_or_default(), raw_rows))
        }));

        let (headers, raw_rows) = match result {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: format!("could not read Avro file: {e}"),
                })
            }
            Err(_) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: "the Avro decoding library panicked on this file (it's likely corrupt or malformed) — \
                              this was caught to avoid crashing the process, but the file could not be parsed"
                        .into(),
                })
            }
        };

        if headers.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "Avro file has no records".into(),
            });
        }

        report.rows_in = raw_rows.len();
        let mut table = TidyTable::new(headers).with_source(filename.to_string());
        for row in raw_rows {
            table.push_row(row);
        }
        table.normalize_row_widths();
        report.rows_out = table.rows.len();

        // Avro already carries its own typed writer schema — no per-cell
        // type ambiguity to resolve the way CSV/fixed-width/PDF have.
        // This field exists purely so with_resolver/new() keep the same
        // shape as every other parser in the workspace.
        let _ = &self.resolver;

        Ok(ParseOutcome { tables: vec![table], report })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_scores_the_avro_magic_header_regardless_of_filename() {
        let mut bytes = b"Obj\x01".to_vec();
        bytes.extend_from_slice(&[0u8; 64]);
        let parser = AvroParser::new();
        assert!(parser.sniff(&bytes, None) > 0.5);
        assert!(parser.sniff(&bytes, Some("data.avro")) > 0.8);
    }

    #[test]
    fn sniff_rejects_content_without_the_magic_header_even_with_a_matching_extension() {
        let parser = AvroParser::new();
        assert!(parser.sniff(b"not an avro file at all, just plain text", Some("data.avro")) < 0.5);
    }

    #[test]
    fn parse_rejects_non_avro_bytes_cleanly() {
        let parser = AvroParser::new();
        let result = parser.parse(b"definitely not an avro file", "fake.avro", &ParseOptions::new());
        assert!(result.is_err());
    }

    #[test]
    fn a_union_wrapped_optional_field_unwraps_to_its_inner_value() {
        let inner = Value::Union(1, Box::new(Value::Long(42)));
        assert_eq!(avro_value_to_tidy(&inner), TidyValue::Int(42));
        let null_case = Value::Union(0, Box::new(Value::Null));
        assert_eq!(avro_value_to_tidy(&null_case), TidyValue::Null);
    }

    #[test]
    fn epoch_day_zero_is_january_first_1970() {
        assert_eq!(date_from_epoch_days(0), "1970-01-01");
    }
}
