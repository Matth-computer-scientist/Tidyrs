//! JSON/XML/YAML parsing for tidyloom — **experimental** in this version.
//!
//! Parsing itself (serde_json for JSON, a small tolerant tree-builder for
//! XML, serde_yaml for YAML) is solid. What's explicitly *not* attempted
//! here is a fully general, lossless model of arbitrarily inconsistent
//! nested structures: flattening is a simple, documented dot-notation +
//! array-join strategy (see [`flatten`]). Genuinely ambiguous cases (a
//! column whose type can't be determined with confidence) are exactly what
//! `tidyrs_core::heuristics` is the extension point for.
//!
//! YAML is parsed straight into a `serde_json::Value` (serde_yaml can
//! deserialize into any `serde::Deserialize` target, and `Value`'s impl is
//! fully self-describing) so it reuses the exact same flattening,
//! record-extraction, and typing pass as JSON, rather than a separate code
//! path that would have to be kept in sync by hand.

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
    Yaml,
}

/// A plain scan for `key: value` shape, not a YAML parser — a leading
/// `- ` list-item marker is stripped first since a sequence of mappings
/// (`- name: Alice`) is the single most common table-like YAML shape.
/// Deliberately conservative about what counts as a key: a key containing
/// a space or a quote is skipped rather than handled, since disambiguating
/// `Note: see below` (prose) from `My Key: value` (quoted/spaced YAML key)
/// from a plain scan isn't reliable — the ratio-based caller tolerates a
/// few lines this misses.
fn is_yaml_key_line(line: &str) -> bool {
    let content = line.trim_start();
    let content = content.strip_prefix("- ").unwrap_or(content).trim_start();
    let Some(colon_idx) = content.find(':') else {
        return false;
    };
    let key = &content[..colon_idx];
    if key.is_empty() || key.contains([' ', '"', '\'']) {
        return false;
    }
    let after = &content[colon_idx + 1..];
    after.is_empty() || after.starts_with(' ') || after.starts_with('\t')
}

fn is_yaml_list_item(line: &str) -> bool {
    let t = line.trim_start();
    t == "-" || t.starts_with("- ")
}

/// Two signals, both required: most sampled lines have to *look* like YAML
/// (the `key:` / `- item` shape scanned above), and the sample has to
/// actually parse as a YAML mapping or sequence rather than a bare scalar.
/// Neither alone is enough — YAML has no unique leading character the way
/// JSON (`{`/`[`) or XML (`<`) does, so a syntax-only scan would false-
/// positive on any prose that happens to contain a colon, while
/// `serde_yaml` alone happily parses nearly *any* text as a one-line
/// string scalar (which is exactly why the match below only accepts
/// `Mapping`/`Sequence`, never a scalar result).
fn looks_like_yaml_content(text: &str) -> bool {
    let lines = tidyrs_core::representative_lines(text, 20);
    let candidates: Vec<&str> = lines
        .into_iter()
        .map(|l| l.trim_end())
        .filter(|l| {
            let t = l.trim_start();
            !t.is_empty() && !t.starts_with('#') && t != "---" && t != "..."
        })
        .collect();
    if candidates.len() < 2 {
        return false;
    }
    let matching = candidates.iter().filter(|l| is_yaml_key_line(l) || is_yaml_list_item(l)).count();
    let ratio = matching as f32 / candidates.len() as f32;
    if ratio < 0.6 {
        return false;
    }
    matches!(
        serde_yaml::from_str::<serde_yaml::Value>(text),
        Ok(serde_yaml::Value::Mapping(_)) | Ok(serde_yaml::Value::Sequence(_))
    )
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
        if lower.ends_with(".yaml") || lower.ends_with(".yml") {
            return Some(Kind::Yaml);
        }
    }
    let text = String::from_utf8_lossy(bytes);
    match text.trim_start().chars().next() {
        Some('{') | Some('[') => return Some(Kind::Json),
        Some('<') => return Some(Kind::Xml),
        _ => {}
    }
    if looks_like_yaml_content(&text) {
        return Some(Kind::Yaml);
    }
    None
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
            // An extension gives as strong a signal here as it does for
            // JSON/XML. Content-only relies on the fuzzier heuristic in
            // looks_like_yaml_content (YAML has no unique leading
            // character the way `{`/`[`/`<` give JSON/XML), so it's scored
            // a bit lower — still clearly ahead of csv/fixed on genuine
            // YAML content, but behind an extension-confirmed guess.
            Some(Kind::Yaml) => {
                let has_yaml_extension = filename.is_some_and(|n| {
                    let lower = n.to_ascii_lowercase();
                    lower.ends_with(".yaml") || lower.ends_with(".yml")
                });
                if has_yaml_extension {
                    0.65
                } else {
                    0.55
                }
            }
            None => 0.0,
        }
    }

    fn parse(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let kind = detect_kind(bytes, Some(filename)).ok_or_else(|| TidyError::Parse {
            format: self.format_name().into(),
            message: "input is neither valid JSON, XML, nor YAML".into(),
        })?;

        let format_label = match kind {
            Kind::Json => "json",
            Kind::Xml => "xml",
            Kind::Yaml => "yaml",
        };
        let mut report = CleaningReport::new(filename, format_label);
        report.warning(
            "json/xml/yaml support is experimental in this version: flattening uses a simple documented strategy, not a fully general one (see README)"
                .to_string(),
        );

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
            Kind::Yaml => serde_yaml::from_str(&text).map_err(|e| TidyError::Parse {
                format: self.format_name().into(),
                message: format!("invalid YAML: {e}"),
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

        let flattened: Vec<std::collections::BTreeMap<String, TidyValue>> = records.iter().flat_map(|r| flatten_record(r, &cfg)).collect();
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

        Ok(ParseOutcome { tables: vec![table], report })
    }
}
