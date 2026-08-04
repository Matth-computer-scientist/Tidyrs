use thiserror::Error;

#[derive(Debug, Error)]
pub enum TidyError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("no parser could confidently handle this input (format detection failed)")]
    UnknownFormat,

    #[error("failed to parse {format}: {message}")]
    Parse { format: String, message: String },

    #[error("export error: {0}")]
    Export(String),

    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type TidyResult<T> = Result<T, TidyError>;
