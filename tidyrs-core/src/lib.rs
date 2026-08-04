//! Core crate for tidyloom: the shared data model, the `TidyParser` trait
//! every format crate implements, format detection, the cleaning report,
//! and the export layer (CSV/JSON/Parquet). Format-specific parsing lives
//! in sibling crates (`tidyrs-csv`, `tidyrs-xlsx`, ...) and depends on this
//! crate — never the other way around.

pub mod detect;
pub mod error;
pub mod export;
pub mod heuristics;
pub mod options;
pub mod parser;
pub mod report;
pub mod schema;
pub mod table;
pub mod typing;
pub mod value;

pub use detect::{Detection, FormatRegistry};
pub use error::{TidyError, TidyResult};
pub use heuristics::{AmbiguityResolver, ColumnTypeGuess, LlmAmbiguityResolver, RuleBasedResolver};
#[cfg(feature = "llm")]
pub use heuristics::HttpLlmResolver;
pub use options::ParseOptions;
pub use parser::{ParseOutcome, TidyParser};
pub use report::{CleaningNote, CleaningReport, Severity};
pub use schema::{validate, ColumnSchema, ExpectedType, Schema, ValidationIssue, ValidationReport};
pub use typing::{type_columns, TypedColumns};
pub use table::TidyTable;
pub use value::TidyValue;
