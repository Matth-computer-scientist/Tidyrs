use serde::{Deserialize, Serialize};
use std::fmt;

/// A single normalized cell value. All format parsers converge on this type
/// before the data leaves their crate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TidyValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

/// True if `s` (after an optional leading `-`) starts with a `0` that's
/// immediately followed by another digit — `"00501"`, `"007"`. No real
/// integer or decimal literal is ever written with a redundant leading
/// zero, but the *representation* — how many digits, whether there was a
/// leading zero at all — carries real meaning for the codes that actually
/// look like this in practice: postal codes, padded serial/account
/// numbers, phone extensions. `str::parse` doesn't care and would happily
/// accept `"00501"` as `501`, silently discarding that meaning — found via
/// external QA testing as real, silent data corruption (a postal code
/// becoming a different, shorter number with no warning), not a cosmetic
/// issue. `"0.5"` is unaffected (the character after the leading `0` is
/// `.`, not a digit) since that's a completely ordinary decimal.
pub fn has_meaningful_leading_zero(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    let bytes = digits.as_bytes();
    bytes.len() > 1 && bytes[0] == b'0' && bytes[1].is_ascii_digit()
}

impl TidyValue {
    /// Best-effort type inference from a raw string cell. Used by parsers
    /// that read everything as text (CSV, fixed-width, PDF) before handing
    /// off to the normalization layer.
    pub fn infer_from_str(raw: &str) -> TidyValue {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return TidyValue::Null;
        }
        if !has_meaningful_leading_zero(trimmed) {
            if let Ok(i) = trimmed.parse::<i64>() {
                return TidyValue::Int(i);
            }
            if let Ok(f) = trimmed.parse::<f64>() {
                return TidyValue::Float(f);
            }
        }
        match trimmed.to_ascii_lowercase().as_str() {
            "true" | "yes" => return TidyValue::Bool(true),
            "false" | "no" => return TidyValue::Bool(false),
            _ => {}
        }
        TidyValue::Text(trimmed.to_string())
    }

    pub fn as_export_string(&self) -> String {
        match self {
            TidyValue::Null => String::new(),
            TidyValue::Bool(b) => b.to_string(),
            TidyValue::Int(i) => i.to_string(),
            TidyValue::Float(f) => f.to_string(),
            TidyValue::Text(s) => s.clone(),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, TidyValue::Null)
    }
}

impl fmt::Display for TidyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_export_string())
    }
}
