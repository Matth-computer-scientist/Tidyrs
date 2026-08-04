use crate::error::TidyResult;
use crate::options::ParseOptions;
use crate::report::CleaningReport;
use crate::table::TidyTable;

/// Result of a successful parse: one or more normalized tables (multiple
/// for formats like multi-sheet Excel) plus the audit trail of what was
/// detected and fixed.
pub struct ParseOutcome {
    pub tables: Vec<TidyTable>,
    pub report: CleaningReport,
}

/// The contract every format module implements. Adding a new input format
/// to tidyloom means writing a new crate that implements this trait — the
/// core crate and the CLI never need to change.
pub trait TidyParser {
    /// Stable identifier for this parser, e.g. "csv", "xlsx".
    fn format_name(&self) -> &'static str;

    /// Confidence (0.0-1.0) that this parser can handle `bytes`. Used by the
    /// format detector to pick a parser independently of file extension.
    /// `filename` is an optional hint (extension, name) that MAY be used to
    /// break ties but must never be the only signal.
    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32;

    /// Parse the raw bytes into normalized tables.
    fn parse(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome>;
}
