//! Configurable flattening of arbitrarily-nested `serde_json::Value` trees
//! into flat `key -> TidyValue` maps that can be unioned into a table.
//!
//! Two array strategies, controlled by `FlattenConfig::explode_arrays`:
//! - `false` (default): arrays of scalars are joined into one delimited
//!   text value; arrays of objects/nested arrays are flattened under
//!   index-suffixed keys (`items[0].sku`, `items[1].sku`, ...) so
//!   structure is preserved without changing the record's row count.
//! - `true`: arrays of objects/nested arrays are *exploded* — each
//!   element produces its own row, combined with the rest of the record's
//!   fields (Cartesian product if a record has more than one such array).
//!   Scalar-only arrays are still joined either way; exploding those too
//!   would multiply rows based on unrelated data for little benefit.
//!   Cartesian explosion means row count is no longer 1:1 with input
//!   records — this is a deliberate opt-in, not the default.
//!
//! A key that is sometimes an object and sometimes a scalar across
//! different records is handled for free: each record just produces
//! whatever flat keys its own shape implies, and the caller unions all
//! keys across records, padding missing ones with `Null`.

use serde_json::Value;
use std::collections::BTreeMap;
use tidyrs_core::TidyValue;

pub struct FlattenConfig {
    pub separator: String,
    pub array_join_sep: String,
    pub explode_arrays: bool,
}

impl Default for FlattenConfig {
    fn default() -> Self {
        Self {
            separator: ".".to_string(),
            array_join_sep: "; ".to_string(),
            explode_arrays: false,
        }
    }
}

type Row = BTreeMap<String, TidyValue>;

/// Flattens one JSON record into one or more rows. Returns more than one
/// row only when `cfg.explode_arrays` is set and the record contains at
/// least one array of objects/nested arrays.
pub fn flatten_record(value: &Value, cfg: &FlattenConfig) -> Vec<Row> {
    flatten_into(value, "", cfg)
}

fn single(prefix: &str, v: TidyValue) -> Vec<Row> {
    let mut row = Row::new();
    row.insert(prefix.to_string(), v);
    vec![row]
}

/// Combines every row in `current` with every row in `additions`,
/// producing a Cartesian product (this is what lets a record with two
/// independent exploded arrays produce `len(a) * len(b)` rows).
fn cartesian_merge(current: Vec<Row>, additions: Vec<Row>) -> Vec<Row> {
    if additions.is_empty() {
        return current;
    }
    let mut out = Vec::with_capacity(current.len() * additions.len());
    for c in &current {
        for a in &additions {
            let mut merged = c.clone();
            merged.extend(a.clone());
            out.push(merged);
        }
    }
    out
}

fn flatten_into(value: &Value, prefix: &str, cfg: &FlattenConfig) -> Vec<Row> {
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                return single(prefix, TidyValue::Null);
            }
            let mut results = vec![Row::new()];
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}{}{k}", cfg.separator)
                };
                let variants = flatten_into(v, &key, cfg);
                results = cartesian_merge(results, variants);
            }
            results
        }
        Value::Array(items) => {
            let all_scalar = items.iter().all(|v| !matches!(v, Value::Object(_) | Value::Array(_)));
            if all_scalar {
                let joined = items.iter().map(scalar_to_string).collect::<Vec<_>>().join(&cfg.array_join_sep);
                single(prefix, TidyValue::Text(joined))
            } else if cfg.explode_arrays {
                if items.is_empty() {
                    return single(prefix, TidyValue::Null);
                }
                items.iter().flat_map(|item| flatten_into(item, prefix, cfg)).collect()
            } else {
                let mut merged = Row::new();
                for (i, item) in items.iter().enumerate() {
                    let key = format!("{prefix}[{i}]");
                    for variant in flatten_into(item, &key, cfg) {
                        merged.extend(variant);
                    }
                }
                vec![merged]
            }
        }
        other => single(prefix, json_scalar_to_tidy(other)),
    }
}

fn scalar_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn json_scalar_to_tidy(v: &Value) -> TidyValue {
    match v {
        Value::Null => TidyValue::Null,
        Value::Bool(b) => TidyValue::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                TidyValue::Int(i)
            } else {
                // Requires the "arbitrary_precision" feature on
                // serde_json (see the workspace Cargo.toml's comment):
                // without it, a JSON integer literal too big for i64/u64
                // is already lossily converted to f64 by serde_json's
                // own parser before this function ever runs — no
                // post-processing here could recover the original
                // digits. With it, n.to_string() reproduces the exact
                // source text, so a whole number that merely overflowed
                // i64 (not a genuine decimal/exponent literal) can be
                // kept as Text instead of silently rounded through f64 —
                // same corruption class, same fix, as
                // TidyValue::looks_like_a_whole_number in tidyrs-core.
                let text = n.to_string();
                if tidyrs_core::looks_like_a_whole_number(&text) {
                    TidyValue::Text(text)
                } else {
                    TidyValue::Float(n.as_f64().unwrap_or(0.0))
                }
            }
        }
        Value::String(s) => TidyValue::infer_from_str(s),
        other => TidyValue::Text(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_integer_literal_too_big_for_i64_keeps_its_exact_digits() {
        // Regression (found via external QA testing): serde_json's own
        // Number type parses an integer literal too big for i64/u64
        // straight into a lossy f64 *at parse time*, unless the
        // "arbitrary_precision" feature is enabled (see the workspace
        // Cargo.toml) — confirmed directly, without that feature this
        // exact literal becomes Number(1e+26) before any of tidyloom's
        // own code runs. "99999999999999999999999999" (26 digits) must
        // not silently become "100000000000000000000000000".
        let v: serde_json::Value = serde_json::from_str("99999999999999999999999999").unwrap();
        assert_eq!(json_scalar_to_tidy(&v), TidyValue::Text("99999999999999999999999999".to_string()));
    }

    #[test]
    fn an_ordinary_integer_still_becomes_a_native_int() {
        let v: serde_json::Value = serde_json::from_str("42").unwrap();
        assert_eq!(json_scalar_to_tidy(&v), TidyValue::Int(42));
    }

    #[test]
    fn i64_min_still_becomes_a_native_int_not_text() {
        let v: serde_json::Value = serde_json::from_str("-9223372036854775808").unwrap();
        assert_eq!(json_scalar_to_tidy(&v), TidyValue::Int(i64::MIN));
    }

    #[test]
    fn a_genuine_decimal_still_becomes_a_native_float() {
        let v: serde_json::Value = serde_json::from_str("10.5").unwrap();
        assert_eq!(json_scalar_to_tidy(&v), TidyValue::Float(10.5));
    }

    #[test]
    fn a_genuine_scientific_notation_literal_still_becomes_a_native_float() {
        // 1e300 has no exact integer representation at all (unlike the
        // oversized-integer case above) — a real float, not a whole
        // number that merely overflowed i64, so the f64 path is correct
        // here and must not be redirected to Text.
        let v: serde_json::Value = serde_json::from_str("1e300").unwrap();
        assert_eq!(json_scalar_to_tidy(&v), TidyValue::Float(1e300));
    }
}
