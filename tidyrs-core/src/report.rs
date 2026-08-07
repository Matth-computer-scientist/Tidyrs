use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
}

/// One recorded correction or observation made while cleaning a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleaningNote {
    pub severity: Severity,
    pub message: String,
}

impl CleaningNote {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            message: message.into(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
        }
    }
}

/// Per-file audit trail: what format was detected and what was fixed along
/// the way. Meant to be surfaced to the user (CLI prints it, or it can be
/// exported as JSON) so cleaning stays auditable rather than a black box.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleaningReport {
    pub source_file: String,
    pub detected_format: String,
    pub rows_in: usize,
    pub rows_out: usize,
    pub notes: Vec<CleaningNote>,
}

impl CleaningReport {
    pub fn new(source_file: impl Into<String>, detected_format: impl Into<String>) -> Self {
        Self {
            source_file: source_file.into(),
            detected_format: detected_format.into(),
            rows_in: 0,
            rows_out: 0,
            notes: Vec::new(),
        }
    }

    pub fn note(&mut self, note: CleaningNote) {
        self.notes.push(note);
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.note(CleaningNote::info(message));
    }

    pub fn warning(&mut self, message: impl Into<String>) {
        self.note(CleaningNote::warning(message));
    }
}
