//! Optional schema validation: declare what a table's columns are
//! supposed to look like (type, nullability, and — in strict mode —
//! exactly which columns are allowed) and check a cleaned `TidyTable`
//! against it. This is what turns tidyloom from "a cleaner" into
//! something that can act as a data-quality gate in a pipeline: fail the
//! build when a source file silently changes shape, instead of quietly
//! shipping bad data downstream.
//!
//! Schemas are plain, serializable data (see [`Schema`]) so they can be
//! authored as a JSON file and loaded with [`Schema::from_json`] — the
//! CLI's `--schema` flag does exactly that.

use crate::table::TidyTable;
use crate::value::TidyValue;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExpectedType {
    Integer,
    Float,
    Boolean,
    Date,
    Text,
    /// No type constraint — only presence/nullability is checked.
    Any,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub name: String,
    #[serde(rename = "type")]
    pub expected_type: ExpectedType,
    /// Whether a null/empty value is acceptable. Defaults to `true`
    /// (nullable) so a minimal schema doesn't accidentally reject every
    /// sparse column.
    #[serde(default = "default_true")]
    pub nullable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Schema {
    pub columns: Vec<ColumnSchema>,
    /// When true, any table column not declared in `columns` is itself a
    /// violation. Defaults to false: by default a schema only constrains
    /// the columns it mentions and says nothing about the rest.
    #[serde(default)]
    pub strict: bool,
}

impl Schema {
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// One validation failure. `row` is `None` for table-level issues (a
/// declared column is entirely missing, or — in strict mode — the table
/// has a column the schema doesn't know about); `Some(i)` for a specific
/// row's cell.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    pub row: Option<usize>,
    pub column: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ValidationReport {
    pub total_rows: usize,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

fn looks_like_date(s: &str) -> bool {
    let digits_and_seps = s.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '/' || c == '.');
    let has_two_seps = s.chars().filter(|&c| c == '-' || c == '/' || c == '.').count() >= 2;
    digits_and_seps && has_two_seps && s.len() >= 6 && s.len() <= 10
}

fn type_matches(value: &TidyValue, expected: ExpectedType) -> bool {
    match expected {
        ExpectedType::Any | ExpectedType::Text => true,
        ExpectedType::Integer => matches!(value, TidyValue::Int(_)),
        ExpectedType::Float => matches!(value, TidyValue::Int(_) | TidyValue::Float(_)),
        ExpectedType::Boolean => matches!(value, TidyValue::Bool(_)),
        ExpectedType::Date => matches!(value, TidyValue::Text(s) if looks_like_date(s)),
    }
}

/// Checks every declared column of `schema` against `table`, collecting
/// every violation found (does not stop at the first one — a caller
/// wants the full picture for a report, not just a pass/fail bit).
pub fn validate(table: &TidyTable, schema: &Schema) -> ValidationReport {
    let mut report = ValidationReport {
        total_rows: table.rows.len(),
        issues: Vec::new(),
    };

    let col_index: HashMap<&str, usize> = table.headers.iter().enumerate().map(|(i, h)| (h.as_str(), i)).collect();

    for col in &schema.columns {
        if !col_index.contains_key(col.name.as_str()) {
            report.issues.push(ValidationIssue {
                row: None,
                column: col.name.clone(),
                message: "column declared in schema is missing from the table".to_string(),
            });
        }
    }

    if schema.strict {
        let known: HashSet<&str> = schema.columns.iter().map(|c| c.name.as_str()).collect();
        for h in &table.headers {
            if !known.contains(h.as_str()) {
                report.issues.push(ValidationIssue {
                    row: None,
                    column: h.clone(),
                    message: "column present in the table but not declared in the schema (strict mode)".to_string(),
                });
            }
        }
    }

    for col in &schema.columns {
        let Some(&idx) = col_index.get(col.name.as_str()) else {
            continue;
        };
        for (row_i, row) in table.rows.iter().enumerate() {
            let Some(value) = row.get(idx) else { continue };
            if value.is_null() {
                if !col.nullable {
                    report.issues.push(ValidationIssue {
                        row: Some(row_i),
                        column: col.name.clone(),
                        message: "unexpected null/empty value in a non-nullable column".to_string(),
                    });
                }
                continue;
            }
            if !type_matches(value, col.expected_type) {
                report.issues.push(ValidationIssue {
                    row: Some(row_i),
                    column: col.name.clone(),
                    message: format!("expected {:?}, got {value:?}", col.expected_type),
                });
            }
        }
    }

    report
}
