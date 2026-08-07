//! Fixed-width and whitespace-separated semi-structured text for tidyloom.
//!
//! Two sub-strategies, chosen via the `mode` option (`"fixed"` or
//! `"whitespace"`, default `"fixed"`):
//! - `fixed`: infer column boundaries from character positions that are
//!   whitespace on every sampled line (the classic column-alignment
//!   heuristic used by tools like pandas' `read_fwf`).
//! - `whitespace`: treat each line as one record, fields separated by any
//!   run of whitespace (typical of ad-hoc log lines) — field count may
//!   vary per line.

use tidyrs_core::{AmbiguityResolver, CleaningReport, ParseOptions, ParseOutcome, RuleBasedResolver, TidyError, TidyParser, TidyResult, TidyTable};

pub struct FixedWidthParser {
    resolver: Box<dyn AmbiguityResolver>,
}

impl FixedWidthParser {
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

impl Default for FixedWidthParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns (start, end) character spans for each inferred field, based on
/// columns that are whitespace across every sampled line.
fn infer_column_spans(lines: &[&str]) -> Vec<(usize, usize)> {
    let max_len = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    if max_len == 0 {
        return vec![];
    }
    let chars: Vec<Vec<char>> = lines.iter().map(|l| l.chars().collect()).collect();

    let mut is_gap = vec![true; max_len];
    for row in &chars {
        for (pos, gap) in is_gap.iter_mut().enumerate() {
            let c = row.get(pos).copied().unwrap_or(' ');
            if !c.is_whitespace() {
                *gap = false;
            }
        }
    }

    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (pos, gap) in is_gap.iter().enumerate() {
        if !gap {
            if start.is_none() {
                start = Some(pos);
            }
        } else if let Some(s) = start.take() {
            spans.push((s, pos));
        }
    }
    if let Some(s) = start {
        spans.push((s, max_len));
    }
    spans
}

fn extract_span(line: &str, span: (usize, usize)) -> String {
    line.chars().skip(span.0).take(span.1 - span.0).collect::<String>().trim().to_string()
}

fn report_ambiguous_columns(report: &mut CleaningReport, ambiguous: &[(String, tidyrs_core::ColumnTypeGuess, f32)]) {
    for (col, guess, confidence) in ambiguous {
        report.info(format!(
            "column '{col}': type is ambiguous (best guess: {guess:?}, confidence {confidence:.2}) — kept per-cell inference; \
             a stronger AmbiguityResolver (e.g. HttpLlmResolver) may resolve this more consistently"
        ));
    }
}

impl TidyParser for FixedWidthParser {
    fn format_name(&self) -> &'static str {
        "fixed"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let mut score: f32 = 0.0;
        if let Some(name) = filename {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".txt") || lower.ends_with(".log") {
                score += 0.2;
            }
        }
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);

        // Random/binary content decoded via from_utf8_lossy is riddled with
        // U+FFFD replacement characters and control bytes that happen to
        // still contain a handful of real newline (0x0A) bytes purely by
        // chance — which was enough, on a small sample, to occasionally
        // pass the line-count and whitespace-alignment checks below and
        // get misdetected as a legitimate fixed-width file. Reject content
        // that isn't overwhelmingly printable/whitespace before doing any
        // of that heuristic work; genuine text files should have ~0% junk.
        let total_chars = text.chars().count();
        if total_chars == 0 {
            return score;
        }
        let junk_chars = text
            .chars()
            .filter(|&c| c == '\u{FFFD}' || (c.is_control() && c != '\n' && c != '\r' && c != '\t'))
            .count();
        if junk_chars as f32 / total_chars as f32 > 0.01 {
            return 0.0;
        }

        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(20).collect();
        if lines.len() < 2 {
            return score;
        }
        // No comma/semicolon/tab/pipe present at all is a decent signal
        // this isn't disguised CSV.
        let has_delim = lines
            .iter()
            .any(|l| l.contains(',') || l.contains(';') || l.contains('\t') || l.contains('|'));
        if !has_delim {
            score += 0.3;
        }
        // Multiple whitespace-separated tokens on most lines.
        let multi_token = lines.iter().filter(|l| l.split_whitespace().count() >= 2).count();
        if multi_token as f32 / lines.len() as f32 > 0.7 {
            score += 0.2;
        }
        score.min(0.6) // stay below CSV/xlsx confidence when genuinely ambiguous
    }

    fn parse(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let mut report = CleaningReport::new(filename, self.format_name());
        let text = String::from_utf8_lossy(bytes).into_owned();
        let all_lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        if all_lines.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "no non-empty lines found".into(),
            });
        }
        let mode = options.get_or("mode", "fixed");
        let has_header = options.get_bool("has_header", false);

        let mut table = if mode == "whitespace" {
            report.info("mode=whitespace: splitting each line on runs of whitespace".to_string());
            let field_counts: Vec<usize> = all_lines.iter().map(|l| l.split_whitespace().count()).collect();
            let width = *field_counts.iter().max().unwrap_or(&0);

            let headers: Vec<String> = if has_header {
                all_lines[0].split_whitespace().map(|s| s.to_string()).collect()
            } else {
                (0..width).map(|i| format!("field_{}", i + 1)).collect()
            };

            let data_lines = if has_header { &all_lines[1..] } else { &all_lines[..] };
            report.rows_in = data_lines.len();
            let mut ragged = 0usize;
            let mut raw_rows: Vec<Vec<String>> = Vec::with_capacity(data_lines.len());
            for line in data_lines {
                let mut tokens: Vec<String> = line.split_whitespace().map(|t| t.to_string()).collect();
                if tokens.len() != width {
                    ragged += 1;
                }
                tokens.resize(width, String::new());
                tokens.truncate(width);
                raw_rows.push(tokens);
            }
            if ragged > 0 {
                report.warning(format!("{ragged} line(s) had a different token count than the inferred field width"));
            }

            let typed = tidyrs_core::type_columns(&headers, &raw_rows, self.resolver.as_ref());
            report_ambiguous_columns(&mut report, &typed.ambiguous_columns);
            let mut table = TidyTable::new(headers).with_source(filename.to_string());
            table.rows = typed.rows;
            table
        } else {
            let sample: Vec<&str> = all_lines.iter().take(200).copied().collect();
            let spans = infer_column_spans(&sample);
            if spans.is_empty() {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: "could not infer any fixed-width column boundaries".into(),
                });
            }
            report.info(format!("inferred {} fixed-width column(s) from whitespace alignment", spans.len()));

            let headers: Vec<String> = if has_header {
                spans.iter().map(|&s| extract_span(all_lines[0], s)).collect()
            } else {
                (0..spans.len()).map(|i| format!("field_{}", i + 1)).collect()
            };

            let data_lines = if has_header { &all_lines[1..] } else { &all_lines[..] };
            report.rows_in = data_lines.len();
            let raw_rows: Vec<Vec<String>> = data_lines
                .iter()
                .map(|line| spans.iter().map(|&s| extract_span(line, s)).collect())
                .collect();

            let typed = tidyrs_core::type_columns(&headers, &raw_rows, self.resolver.as_ref());
            report_ambiguous_columns(&mut report, &typed.ambiguous_columns);
            let mut table = TidyTable::new(headers).with_source(filename.to_string());
            table.rows = typed.rows;
            table
        };

        table.normalize_row_widths();
        report.rows_out = table.rows.len();

        Ok(ParseOutcome { tables: vec![table], report })
    }
}
