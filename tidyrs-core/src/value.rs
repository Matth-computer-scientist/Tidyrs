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

impl TidyValue {
    /// Best-effort type inference from a raw string cell. Used by parsers
    /// that read everything as text (CSV, fixed-width, PDF) before handing
    /// off to the normalization layer.
    pub fn infer_from_str(raw: &str) -> TidyValue {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return TidyValue::Null;
        }
        if let Ok(i) = trimmed.parse::<i64>() {
            return TidyValue::Int(i);
        }
        if let Ok(f) = trimmed.parse::<f64>() {
            return TidyValue::Float(f);
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
