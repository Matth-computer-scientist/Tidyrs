use crate::value::TidyValue;
use serde::{Deserialize, Serialize};

/// The common tabular representation every parser must produce. This is the
/// single point of convergence that lets format-specific crates stay fully
/// independent of one another.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TidyTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<TidyValue>>,
    /// Name of the logical unit the table came from (e.g. a sheet name for
    /// Excel, or the source file name for single-table formats).
    pub source: Option<String>,
}

impl TidyTable {
    pub fn new(headers: Vec<String>) -> Self {
        Self {
            headers,
            rows: Vec::new(),
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn push_row(&mut self, row: Vec<TidyValue>) {
        self.rows.push(row);
    }

    /// Pads or truncates every row so its length matches the header count.
    /// This is what lets ragged/malformed input survive parsing instead of
    /// aborting the whole file.
    pub fn normalize_row_widths(&mut self) {
        let width = self.headers.len();
        for row in &mut self.rows {
            match row.len().cmp(&width) {
                std::cmp::Ordering::Less => row.resize(width, TidyValue::Null),
                std::cmp::Ordering::Greater => row.truncate(width),
                std::cmp::Ordering::Equal => {}
            }
        }
    }
}
