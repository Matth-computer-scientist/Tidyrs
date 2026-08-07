//! Apache Parquet reading for tidyloom — **experimental**.
//!
//! `tidyrs-core::export` can already *write* Parquet (a low-level, hand-
//! rolled column writer, since it only ever needs to serialize
//! `TidyTable`'s five simple value types). Reading is the opposite
//! problem — an *arbitrary* Parquet file can carry a much richer typed
//! schema than `TidyValue` has variants for — so this crate goes through
//! `parquet`'s own `arrow` feature (decoding into Arrow `RecordBatch`es)
//! rather than the low-level column-chunk API, and converts cell-by-cell
//! the same way `tidyrs-orc` does: Boolean/integer/floating-point columns
//! map directly onto `Bool`/`Int`/`Float`; Date/Timestamp/Decimal and
//! nested Struct/List/Map columns render as Arrow's own canonical display
//! text (`arrow_cast::display`) rather than a hand-converted native type
//! or JSON-style dot-notation flattening — a real, documented
//! simplification, not an oversight.
//!
//! Deliberately pinned to the same Arrow major version (53) as
//! `tidyrs-core`'s own `parquet` dependency, so this crate doesn't
//! introduce a second incompatible Arrow version tree the way
//! `tidyrs-orc`'s `orc-rust` (pinned to Arrow 58) had to.

use arrow_array::{Array, RecordBatch};
use arrow_schema::DataType;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::collections::BTreeSet;
use tidyrs_core::{
    AmbiguityResolver, CleaningReport, ParseOptions, ParseOutcome, RuleBasedResolver, TidyError, TidyParser, TidyResult, TidyTable, TidyValue,
};

pub struct ParquetParser {
    resolver: Box<dyn AmbiguityResolver>,
}

impl ParquetParser {
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

impl Default for ParquetParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Parquet's 4-byte magic appears at both the start and the end of a
/// well-formed file (the header confirms the format up front; the
/// trailing copy lets a reader seeking from EOF find the footer without
/// scanning the whole file). Checking the header alone — the same
/// leading-magic approach `tidyrs-xlsx` uses for its ZIP signature — is
/// enough for detection; a genuinely truncated/corrupt footer is exactly
/// what `parquet`'s own parse error surfaces at parse time instead.
const MAGIC: &[u8] = b"PAR1";

fn has_parquet_extension(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".parquet")
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
            // PrimitiveArray<T> generically — see tidyrs-orc's cell_to_tidy
            // for why UInt64 is deliberately excluded from this path.
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
        // Date/Timestamp/Decimal/UInt64/Binary/Struct/List/Map/... — see
        // module docs for why these become Arrow's own display text.
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

impl TidyParser for ParquetParser {
    fn format_name(&self) -> &'static str {
        "parquet"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let mut score: f32 = 0.0;
        if let Some(name) = filename {
            if has_parquet_extension(name) {
                score += 0.3;
            }
        }
        if bytes.starts_with(MAGIC) {
            score += 0.6;
        }
        score.min(1.0)
    }

    fn parse(&self, bytes: &[u8], filename: &str, _options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let mut report = CleaningReport::new(filename, self.format_name());
        report.warning(
            "Parquet *reading* support is experimental in this version: Date/Timestamp/Decimal columns and nested \
             Struct/List/Map columns are rendered as text via Arrow's own display formatting rather than a fully \
             typed conversion — see the tidyrs-parquet module docs for exactly what this does and doesn't handle. \
             (Parquet *writing*, via --output-format parquet, is unaffected and remains fully typed.)"
                .to_string(),
        );

        if !bytes.starts_with(MAGIC) {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "not a Parquet file (missing the 'PAR1' magic header)".into(),
            });
        }

        // The `parquet` crate's arrow-decoding path isn't proptest-
        // hardened in this workspace the way calamine/pdf-extract/
        // rusqlite have been; isolate it behind catch_unwind on
        // principle, consistent with every other parser here that wraps
        // a third-party binary-format decoder.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let builder = ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::copy_from_slice(bytes)).map_err(|e| e.to_string())?;
            let headers: Vec<String> = builder.schema().fields().iter().map(|f| f.name().clone()).collect();
            let reader = builder.build().map_err(|e| e.to_string())?;
            let mut raw_rows: Vec<Vec<TidyValue>> = Vec::new();
            for batch in reader {
                let batch = batch.map_err(|e| e.to_string())?;
                raw_rows.extend(batch_to_rows(&batch));
            }
            Ok::<_, String>((headers, raw_rows))
        }));

        let (headers, raw_rows) = match result {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: format!("could not read Parquet file: {e}"),
                })
            }
            Err(_) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: "the Parquet decoding library panicked on this file (it's likely corrupt or malformed) — \
                              this was caught to avoid crashing the process, but the file could not be parsed"
                        .into(),
                })
            }
        };

        if headers.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "Parquet file has no columns".into(),
            });
        }

        // Deduplicate header names — same approach tidyrs-xlsx/tidyrs-orc
        // use, in case a schema somehow repeats a field name.
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

        // Parquet already carries its own typed schema — no per-cell type
        // ambiguity to resolve the way CSV/fixed-width/PDF have. This
        // field exists purely so with_resolver/new() keep the same shape
        // as every other parser in the workspace.
        let _ = &self.resolver;

        Ok(ParseOutcome { tables: vec![table], report })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_scores_the_parquet_magic_header_regardless_of_filename() {
        let mut bytes = b"PAR1".to_vec();
        bytes.extend_from_slice(&[0u8; 64]);
        let parser = ParquetParser::new();
        assert!(parser.sniff(&bytes, None) > 0.5);
        assert!(parser.sniff(&bytes, Some("data.parquet")) > 0.8);
    }

    #[test]
    fn sniff_rejects_content_without_the_magic_header_even_with_a_matching_extension() {
        let parser = ParquetParser::new();
        assert!(parser.sniff(b"not a parquet file at all, just plain text", Some("data.parquet")) < 0.5);
    }

    #[test]
    fn parse_rejects_non_parquet_bytes_cleanly() {
        let parser = ParquetParser::new();
        let result = parser.parse(b"definitely not a parquet file", "fake.parquet", &ParseOptions::new());
        assert!(result.is_err());
    }
}
