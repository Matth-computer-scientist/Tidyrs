//! JSON/XML parsing for tidyloom — **experimental** in this version.
//!
//! Parsing itself (serde_json for JSON, a small tolerant tree-builder for
//! XML) is solid. What's explicitly *not* attempted here is a fully
//! general, lossless model of arbitrarily inconsistent nested structures:
//! flattening is a simple, documented dot-notation + array-join strategy
//! (see [`flatten`]). Genuinely ambiguous cases (a column whose type can't
//! be determined with confidence) are exactly what `tidyrs_core::heuristics`
//! is the extension point for.

mod flatten;
mod xml;

use flatten::{flatten_record, FlattenConfig};
use serde_json::Value;
use std::collections::BTreeSet;
use tidyrs_core::{CleaningReport, ParseOptions, ParseOutcome, TidyError, TidyParser, TidyResult, TidyTable, TidyValue};

pub struct JsonXmlParser;

impl JsonXmlParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonXmlParser {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(PartialEq)]
enum Kind {
    Json,
    Xml,
}

fn detect_kind(bytes: &[u8], filename: Option<&str>) -> Option<Kind> {
    if let Some(name) = filename {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".json") {
            return Some(Kind::Json);
        }
        if lower.ends_with(".xml") {
            return Some(Kind::Xml);
        }
    }
    let text = String::from_utf8_lossy(bytes);
    let first_non_ws = text.trim_start().chars().next()?;
    match first_non_ws {
        '{' | '[' => Some(Kind::Json),
        '<' => Some(Kind::Xml),
        _ => None,
    }
}

/// Picks the list of "records" to flatten into rows: the root array if the
/// document is one, or the first object property whose value is an array
/// (the common `{"items": [...]}` wrapper shape), or the whole document as
/// a single record otherwise.
fn extract_records(root: Value) -> (Vec<Value>, Option<String>) {
    match root {
        Value::Array(items) => (items, None),
        Value::Object(map) => {
            for (k, v) in map.iter() {
                if let Value::Array(items) = v {
                    return (items.clone(), Some(k.clone()));
                }
            }
            (vec![Value::Object(map)], None)
        }
        other => (vec![other], None),
    }
}

impl TidyParser for JsonXmlParser {
    fn format_name(&self) -> &'static str {
        "json"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        match detect_kind(bytes, filename) {
            Some(Kind::Json) => 0.6,
            Some(Kind::Xml) => 0.55,
            None => 0.0,
        }
    }

    fn parse(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let kind = detect_kind(bytes, Some(filename)).ok_or_else(|| TidyError::Parse {
            format: self.format_name().into(),
            message: "input is neither valid JSON nor XML".into(),
        })?;

        let mut report = CleaningReport::new(filename, if kind == Kind::Json { "json" } else { "xml" });
        report.warning("json/xml support is experimental in this version: flattening uses a simple documented strategy, not a fully general one (see README)".to_string());

        let text = String::from_utf8_lossy(bytes).into_owned();
        let root: Value = match kind {
            Kind::Json => serde_json::from_str(&text).map_err(|e| TidyError::Parse {
                format: self.format_name().into(),
                message: format!("invalid JSON: {e}"),
            })?,
            Kind::Xml => xml::xml_to_value(&text).map_err(|e| TidyError::Parse {
                format: self.format_name().into(),
                message: e,
            })?,
        };

        let (records, wrapper_key) = extract_records(root);
        if let Some(k) = &wrapper_key {
            report.info(format!("used array found under key '{k}' as the row source"));
        }
        report.rows_in = records.len();

        let explode_arrays = options.get_or("array_mode", "join") == "explode";
        let cfg = FlattenConfig {
            separator: options.get_or("separator", ".").to_string(),
            array_join_sep: options.get_or("array_join_sep", "; ").to_string(),
            explode_arrays,
        };

        let flattened: Vec<std::collections::BTreeMap<String, TidyValue>> =
            records.iter().flat_map(|r| flatten_record(r, &cfg)).collect();
        if explode_arrays && flattened.len() != records.len() {
            report.info(format!(
                "array_mode=explode: {} input record(s) expanded into {} row(s)",
                records.len(),
                flattened.len()
            ));
        }

        let mut headers: BTreeSet<String> = BTreeSet::new();
        for rec in &flattened {
            for k in rec.keys() {
                headers.insert(k.clone());
            }
        }
        let headers: Vec<String> = headers.into_iter().collect();

        let inconsistent = flattened.iter().any(|rec| rec.len() != headers.len());
        if inconsistent {
            report.info("records did not all share the same shape; missing fields were filled with null".to_string());
        }

        let mut table = TidyTable::new(headers.clone()).with_source(filename.to_string());
        for rec in &flattened {
            let row: Vec<TidyValue> = headers.iter().map(|h| rec.get(h).cloned().unwrap_or(TidyValue::Null)).collect();
            table.push_row(row);
        }
        report.rows_out = table.rows.len();

        Ok(ParseOutcome {
            tables: vec![table],
            report,
        })
    }
}
