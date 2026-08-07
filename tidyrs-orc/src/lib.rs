//! Apache ORC reading for tidyloom — **experimental**.
//!
//! ORC (like Parquet) has a rich, fully-typed columnar schema — richer
//! than [`TidyValue`]'s five variants. Boolean/integer/floating-point
//! columns map directly onto `Bool`/`Int`/`Float`. Everything else
//! (Date/Timestamp/Decimal, and nested Struct/List/Map columns) is
//! rendered through Arrow's own canonical display formatting
//! (`arrow_cast::display`) into `Text` instead of being hand-converted
//! type by type or flattened: correct and human-readable, but a real,
//! documented simplification — a `Decimal128` value becomes its exact
//! decimal string (not `Float`, to avoid a false precision claim), and a
//! nested `Struct`/`List` becomes Arrow's bracketed/braced text
//! representation rather than dot-notation sub-columns the way
//! `tidyrs-json`'s flattening handles nested JSON/YAML.
//!
//! Reading itself is delegated entirely to `orc-rust` (Apache-2.0,
//! `datafusion-contrib/orc-rust`), which decodes into Arrow
//! `RecordBatch`es directly — chosen over the only other actively-listed
//! ORC crate on crates.io (`orcrs`) for two disqualifying reasons: an
//! "Anti-Capitalist Software License" that explicitly forbids for-profit
//! commercial use (incompatible with this project's MIT/Apache-2.0
//! licensing), and, independent of licensing, real feature gaps
//! (`orcrs`'s own README states floating-point columns, date columns, and
//! Snappy compression — one of ORC's most common compression codecs — are
//! all unsupported).

use arrow_array::{Array, RecordBatch};
use arrow_schema::DataType;
use orc_rust::ArrowReaderBuilder;
use std::collections::BTreeSet;
use tidyrs_core::{
    AmbiguityResolver, CleaningReport, ParseOptions, ParseOutcome, RuleBasedResolver, TidyError, TidyParser, TidyResult, TidyTable, TidyValue,
};

pub struct OrcParser {
    resolver: Box<dyn AmbiguityResolver>,
}

impl OrcParser {
    pub fn new() -> Self {
        Self {
            resolver: Box::new(RuleBasedResolver),
        }
    }

    /// See `tidyrs_csv::CsvParser::with_resolver` — same idea, same
    /// extension point.
    pub fn with_resolver(resolver: Box<dyn AmbiguityResolver>) -> Self {
        Self { resolver }
    }
}

impl Default for OrcParser {
    fn default() -> Self {
        Self::new()
    }
}

/// The fixed 3-byte magic ("ORC") that closes every valid ORC file,
/// immediately followed by a 1-byte postscript length — ORC's footer is
/// at the *end* of the file (it's a self-describing columnar format
/// written in one pass, so metadata can only be finalized once all data
/// has been written), unlike SQLite's or a ZIP-based format's leading
/// header. A short/truncated file can't be distinguished this way, but
/// `orc-rust`'s own parse error handles that case at parse time.
const MAGIC: &[u8] = b"ORC";

fn has_orc_extension(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".orc")
}

fn ends_with_orc_magic(bytes: &[u8]) -> bool {
    // Postscript length (1 byte) + magic (3 bytes) is the minimum valid
    // trailer; tolerate a few bytes of padding some writers add after the
    // magic by scanning the last 16 bytes rather than requiring an exact
    // final-3-bytes match.
    let tail_len = bytes.len().min(16);
    bytes[bytes.len() - tail_len..].windows(MAGIC.len()).any(|w| w == MAGIC)
}

fn cell_to_tidy(column: &dyn Array, row: usize) -> TidyValue {
    if column.is_null(row) {
        return TidyValue::Null;
    }
    match column.data_type() {
        DataType::Boolean => {
            let arr = column.as_any().downcast_ref::<arrow_array::BooleanArray>().unwrap();
            TidyValue::Bool(arr.value(row))
        }
        DataType::Int8 | DataType::Int16 | DataType::Int32 | DataType::Int64 | DataType::UInt8 | DataType::UInt16 | DataType::UInt32 => {
            // Widen every integer width to i64 rather than handling each
            // PrimitiveArray<T> generically — TidyValue::Int is i64 anyway,
            // and ORC integer columns never legitimately need more than
            // that (UInt64 is the one integer type deliberately excluded
            // here: it can hold values that don't fit in i64, so it falls
            // through to the text-formatting path below instead of
            // silently wrapping/truncating).
            let text = arrow_cast::display::array_value_to_string(column, row).unwrap_or_default();
            match text.parse::<i64>() {
                Ok(i) => TidyValue::Int(i),
                Err(_) => TidyValue::Text(text),
            }
        }
        DataType::Float32 | DataType::Float64 => {
            let text = arrow_cast::display::array_value_to_string(column, row).unwrap_or_default();
            match text.parse::<f64>() {
                Ok(f) => TidyValue::Float(f),
                Err(_) => TidyValue::Text(text),
            }
        }
        DataType::Utf8 | DataType::LargeUtf8 => {
            let text = arrow_cast::display::array_value_to_string(column, row).unwrap_or_default();
            TidyValue::infer_from_str(&text)
        }
        // Date/Timestamp/Decimal/UInt64/Struct/List/Map/... — see module
        // docs for why these become Arrow's own display text rather than
        // a hand-converted native TidyValue variant.
        _ => TidyValue::Text(arrow_cast::display::array_value_to_string(column, row).unwrap_or_default()),
    }
}

fn batch_to_rows(batch: &RecordBatch) -> Vec<Vec<TidyValue>> {
    let n_rows = batch.num_rows();
    let columns = batch.columns();
    (0..n_rows)
        .map(|row| columns.iter().map(|col| cell_to_tidy(col.as_ref(), row)).collect())
        .collect()
}

impl TidyParser for OrcParser {
    fn format_name(&self) -> &'static str {
        "orc"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let mut score: f32 = 0.0;
        if let Some(name) = filename {
            if has_orc_extension(name) {
                score += 0.3;
            }
        }
        if bytes.len() >= 4 && ends_with_orc_magic(bytes) {
            score += 0.6;
        }
        score.min(1.0)
    }

    fn parse(&self, bytes: &[u8], filename: &str, _options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let mut report = CleaningReport::new(filename, self.format_name());
        report.warning(
            "ORC support is experimental in this version: Date/Timestamp/Decimal columns and nested Struct/List/Map \
             columns are rendered as text via Arrow's own display formatting rather than a fully typed conversion — \
             see the tidyrs-orc module docs for exactly what this does and doesn't handle."
                .to_string(),
        );

        if bytes.len() < 4 || !ends_with_orc_magic(bytes) {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "not an ORC file (missing the 'ORC' magic trailer)".into(),
            });
        }

        // orc-rust's own error handling for structurally-invalid-but-not-
        // rejected files hasn't been proptest-hardened the way calamine/
        // pdf-extract/rusqlite have been in this workspace; isolate it
        // behind catch_unwind on principle, consistent with every other
        // parser here that wraps a third-party binary-format decoder.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // `ChunkReader` (orc-rust's random-access trait, needed since
            // ORC's footer lives at the end of the file) is implemented
            // for `std::fs::File` and `bytes::Bytes`, not a generic
            // `Read`/`Cursor` — `Bytes` is the in-memory option, matching
            // every other parser here taking `&[u8]` rather than a path.
            let reader = ArrowReaderBuilder::try_new(bytes::Bytes::copy_from_slice(bytes))
                .map_err(|e| e.to_string())?
                .build();
            let mut headers: Vec<String> = Vec::new();
            let mut raw_rows: Vec<Vec<TidyValue>> = Vec::new();
            for batch in reader {
                // The iterator yields ArrowError (batch decoding), distinct
                // from OrcError (file/footer-level errors) above — both
                // collapse to a plain String here rather than threading two
                // separate error types through this closure.
                let batch = batch.map_err(|e| e.to_string())?;
                if headers.is_empty() {
                    headers = batch.schema().fields().iter().map(|f| f.name().clone()).collect();
                }
                raw_rows.extend(batch_to_rows(&batch));
            }
            Ok::<_, String>((headers, raw_rows))
        }));

        let (headers, raw_rows) = match result {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: format!("could not read ORC file: {e}"),
                })
            }
            Err(_) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: "the ORC decoding library panicked on this file (it's likely corrupt or malformed) — \
                              this was caught to avoid crashing the process, but the file could not be parsed"
                        .into(),
                })
            }
        };

        if headers.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "ORC file has no columns or no row batches".into(),
            });
        }

        // Deduplicate header names the same way tidyrs-xlsx does — ORC
        // schemas don't forbid a repeated field name across nested
        // Struct-flattening at the source, though it's rare.
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut deduped_headers = Vec::with_capacity(headers.len());
        for h in headers {
            let mut candidate = h.clone();
            let mut suffix = 1;
            while seen.contains(&candidate) {
                suffix += 1;
                candidate = format!("{h}_{suffix}");
            }
            seen.insert(candidate.clone());
            deduped_headers.push(candidate);
        }

        report.rows_in = raw_rows.len();
        let mut table = TidyTable::new(deduped_headers).with_source(filename.to_string());
        for row in raw_rows {
            table.push_row(row);
        }
        table.normalize_row_widths();
        report.rows_out = table.rows.len();

        // The AmbiguityResolver extension point is wired for parsers that
        // read everything as text first (CSV, fixed-width, PDF) and need
        // to *infer* a column's type. ORC already carries its own typed
        // schema, so there's no ambiguity to resolve here — this field
        // exists purely so with_resolver/new() keep the same shape as
        // every other parser in the workspace.
        let _ = &self.resolver;

        Ok(ParseOutcome { tables: vec![table], report })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_scores_the_orc_magic_trailer_regardless_of_filename() {
        let mut bytes = vec![0u8; 32];
        bytes.extend_from_slice(b"ORC");
        let parser = OrcParser::new();
        assert!(parser.sniff(&bytes, None) > 0.5);
        assert!(parser.sniff(&bytes, Some("data.orc")) > 0.8);
    }

    #[test]
    fn sniff_rejects_content_without_the_magic_trailer_even_with_a_matching_extension() {
        let parser = OrcParser::new();
        assert!(parser.sniff(b"not an orc file at all, just plain text here", Some("data.orc")) < 0.5);
    }

    #[test]
    fn parse_rejects_non_orc_bytes_cleanly() {
        let parser = OrcParser::new();
        let result = parser.parse(b"definitely not an orc file", "fake.orc", &ParseOptions::new());
        assert!(result.is_err());
    }
}
