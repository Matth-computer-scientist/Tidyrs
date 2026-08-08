//! Column-wide type inference: the actual wiring for
//! [`crate::AmbiguityResolver`] into the parsing pipeline.
//!
//! Before this module existed, [`crate::TidyValue::infer_from_str`] was
//! applied per *cell*, independently — so a column of ages like
//! `["30", "41", "N/A", "25"]` would come out as `[Int(30), Int(41),
//! Text("N/A"), Int(25)]`: three different apparent types in one column,
//! nothing tying the decision to the column as a whole, and the
//! `AmbiguityResolver` trait boundary that existed specifically for "what
//! type is this column, really?" was never actually consulted by any
//! parser. This module is that missing connection: parsers that start
//! from raw strings (CSV, fixed-width, PDF — JSON and Excel already carry
//! their own per-cell types from `serde_json`/`calamine`) collect a
//! column's raw values, ask the resolver once what the column's type is,
//! and convert the whole column consistently.

use crate::heuristics::{AmbiguityResolver, ColumnTypeGuess};
use crate::value::TidyValue;

/// Minimum confidence required to commit a column to the resolver's
/// guessed type. Below this, we fall back to the old per-cell inference
/// for that column rather than forcing a low-confidence type onto data
/// that doesn't clearly fit — see [`TypedColumns::ambiguous_columns`].
const CONFIDENCE_THRESHOLD: f32 = 0.7;

pub struct TypedColumns {
    /// Rows in the original row order, each with one [`TidyValue`] per
    /// column.
    pub rows: Vec<Vec<TidyValue>>,
    /// Columns whose resolver confidence fell below
    /// [`CONFIDENCE_THRESHOLD`], along with the resolver's best guess and
    /// confidence anyway — useful for a caller that wants to report "this
    /// one was genuinely unclear" (that's the whole point of the
    /// ambiguity-resolver extension point: some cases plain rules can't
    /// settle, and a caller with a stronger resolver, e.g. an LLM, should
    /// be able to see exactly where the weak default fell down).
    pub ambiguous_columns: Vec<(String, ColumnTypeGuess, f32)>,
    /// Columns that were confidently typed, for reporting.
    pub typed_columns: Vec<(String, ColumnTypeGuess, f32)>,
}

/// Converts one column's raw string values according to `guess`,
/// preserving any individual value that doesn't actually fit the guessed
/// type as `Text` rather than dropping or corrupting it — a column being
/// "mostly integers" doesn't mean every value in it parses as one.
fn convert_column(guess: &ColumnTypeGuess, raw_values: &[String]) -> Vec<TidyValue> {
    raw_values
        .iter()
        .map(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return TidyValue::Null;
            }
            // A leading-zero value ("00501") gets the same "doesn't
            // actually fit the guessed type" treatment as a value that
            // fails to parse outright — see has_meaningful_leading_zero's
            // docs. Without this, a column resolved to Integer/Float
            // because the *majority* of its values were safely numeric
            // would still silently mangle the minority that weren't,
            // exactly the corruption this whole function's docstring
            // already promises not to do.
            match guess {
                ColumnTypeGuess::Integer if !crate::has_meaningful_leading_zero(trimmed) => trimmed
                    .parse::<i64>()
                    .map(TidyValue::Int)
                    .unwrap_or_else(|_| TidyValue::Text(trimmed.to_string())),
                // A whole number that overflowed i64 (see
                // looks_like_a_whole_number's docs) gets the same
                // treatment: a column resolved to Float because most
                // values were genuine decimals must not silently round
                // an oversized integer outlier through f64 either.
                ColumnTypeGuess::Float if !crate::has_meaningful_leading_zero(trimmed) && !crate::looks_like_a_whole_number(trimmed) => trimmed
                    .parse::<f64>()
                    .map(TidyValue::Float)
                    .unwrap_or_else(|_| TidyValue::Text(trimmed.to_string())),
                ColumnTypeGuess::Integer | ColumnTypeGuess::Float => TidyValue::Text(trimmed.to_string()),
                ColumnTypeGuess::Boolean => match trimmed.to_ascii_lowercase().as_str() {
                    "true" | "yes" => TidyValue::Bool(true),
                    "false" | "no" => TidyValue::Bool(false),
                    _ => TidyValue::Text(trimmed.to_string()),
                },
                // No dedicated TidyValue::Date variant exists yet, so a
                // date-classified column still lands as normalized Text
                // — but it's now *consistently* text with that intent
                // recorded, rather than an accident of one cell failing
                // int-parsing.
                ColumnTypeGuess::Date | ColumnTypeGuess::Text => TidyValue::Text(trimmed.to_string()),
            }
        })
        .collect()
}

/// Types every column of `raw_rows` (already padded/truncated to
/// `headers.len()` by the caller — ragged-row handling stays the
/// parser's job, this only handles typing) using `resolver`.
pub fn type_columns(headers: &[String], raw_rows: &[Vec<String>], resolver: &dyn AmbiguityResolver) -> TypedColumns {
    let ncols = headers.len();
    let mut columns_raw: Vec<Vec<String>> = vec![Vec::with_capacity(raw_rows.len()); ncols];
    for row in raw_rows {
        for (i, v) in row.iter().enumerate() {
            if i < ncols {
                columns_raw[i].push(v.clone());
            }
        }
    }

    let mut per_column_values: Vec<Vec<TidyValue>> = Vec::with_capacity(ncols);
    let mut ambiguous_columns = Vec::new();
    let mut typed_columns = Vec::new();

    for (i, header) in headers.iter().enumerate() {
        let (guess, confidence) = resolver.resolve_column_type(header, &columns_raw[i]);

        if confidence < CONFIDENCE_THRESHOLD {
            ambiguous_columns.push((header.clone(), guess, confidence));
            // Fall back to independent per-cell inference: no single
            // type was confident enough to commit the whole column to.
            per_column_values.push(columns_raw[i].iter().map(|s| TidyValue::infer_from_str(s)).collect());
        } else {
            typed_columns.push((header.clone(), guess.clone(), confidence));
            per_column_values.push(convert_column(&guess, &columns_raw[i]));
        }
    }

    let mut rows: Vec<Vec<TidyValue>> = vec![Vec::with_capacity(ncols); raw_rows.len()];
    for column in per_column_values {
        for (row_i, value) in column.into_iter().enumerate() {
            rows[row_i].push(value);
        }
    }

    TypedColumns {
        rows,
        ambiguous_columns,
        typed_columns,
    }
}
