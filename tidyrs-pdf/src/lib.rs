//! PDF table extraction for tidyloom — **experimental proof of concept**,
//! not a stable format in this version.
//!
//! Reconstructing tabular structure from a PDF is one of the hardest
//! problems in this space: PDF has no notion of "table", only positioned
//! glyphs. Dedicated tools (Camelot, Tabula, pdfplumber) exist specifically
//! to attack this and still fail on many real layouts. This crate extracts
//! real glyph positions (see [`glyphs`]) rather than relying on
//! `pdf-extract`'s flattened text output, so whitespace-alignment column
//! inference works from actual geometric spacing instead of character
//! counts — meaning it's no longer limited to monospaced fonts. It still
//! works best on simple, well-aligned text tables and will misfire on
//! multi-line cells, rotated text, or anything visually complex. OCR (for
//! scanned/image PDFs) is explicitly out of scope. Treat this crate's
//! output as a starting point to review, not a guaranteed-correct
//! extraction.

mod glyphs;

use tidyrs_core::{
    AmbiguityResolver, CleaningReport, ParseOptions, ParseOutcome, RuleBasedResolver, TidyError, TidyParser, TidyResult, TidyTable,
};

pub struct PdfParser {
    resolver: Box<dyn AmbiguityResolver>,
}

impl PdfParser {
    pub fn new() -> Self {
        Self { resolver: Box::new(RuleBasedResolver) }
    }

    /// See `tidyrs_csv::CsvParser::with_resolver` — same idea, same
    /// extension point.
    pub fn with_resolver(resolver: Box<dyn AmbiguityResolver>) -> Self {
        Self { resolver }
    }
}

impl Default for PdfParser {
    fn default() -> Self {
        Self::new()
    }
}

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

/// A title line above the real header (e.g. "Quarterly Sales Report")
/// usually breaks whitespace alignment with the rest of the table: its
/// text doesn't line up with the columns below it, so including it in
/// `infer_column_spans` merges what should be separate columns into one
/// wide span. We don't have a header/footer detector as principled as
/// `tidyrs-xlsx`'s (there's no "populated cell count" concept in raw
/// text), so instead we try dropping 0, 1, 2, ... leading lines and keep
/// whichever skip count first reaches the maximum number of inferred
/// columns — a title line disrupting alignment should make the column
/// count go up once it's excluded, and further skips are wasted once it
/// plateaus.
fn find_header_offset(lines: &[&str]) -> usize {
    let max_skip = lines.len().saturating_sub(2).min(3);
    let mut best_skip = 0;
    let mut best_span_count = infer_column_spans(lines).len();
    for skip in 1..=max_skip {
        let span_count = infer_column_spans(&lines[skip..]).len();
        if span_count > best_span_count {
            best_span_count = span_count;
            best_skip = skip;
        }
    }
    best_skip
}

impl TidyParser for PdfParser {
    fn format_name(&self) -> &'static str {
        "pdf"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let mut score: f32 = 0.0;
        if let Some(name) = filename {
            if name.to_ascii_lowercase().ends_with(".pdf") {
                score += 0.3;
            }
        }
        if bytes.starts_with(b"%PDF-") {
            score += 0.6;
        }
        score.min(1.0)
    }

    fn parse(&self, bytes: &[u8], filename: &str, _options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let mut report = CleaningReport::new(filename, self.format_name());
        report.warning(
            "PDF support is experimental in this version: table structure is reconstructed heuristically \
             from text positions and should be reviewed, not trusted blindly. Scanned/image PDFs (OCR) are not supported."
                .to_string(),
        );

        // pdf-extract (and the lopdf it's built on) contain internal
        // `.unwrap()`/`panic!` calls that a malformed-but-not-rejected PDF
        // can hit (confirmed by this crate's proptest robustness suite —
        // corrupted font tables and dangling object references both
        // panic rather than returning an Err). We don't control that
        // third-party code, but a corrupt input file must never take down
        // a process meant to run unattended in a pipeline, so the call is
        // isolated behind `catch_unwind` and turned into a normal parse
        // error instead.
        let extraction = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| glyphs::extract_virtual_lines(bytes)));
        let virtual_lines = match extraction {
            Ok(Ok(lines)) => lines,
            Ok(Err(e)) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: format!("could not extract text (is this a scanned/image PDF? OCR is out of scope): {e}"),
                })
            }
            Err(_) => {
                return Err(TidyError::Parse {
                    format: self.format_name().into(),
                    message: "the PDF text extraction library panicked on this file (it's likely corrupt or malformed) — \
                              this was caught to avoid crashing the process, but the file could not be parsed"
                        .into(),
                })
            }
        };

        let lines: Vec<&str> = virtual_lines.iter().map(|s| s.as_str()).filter(|l| !l.trim().is_empty()).collect();
        if lines.len() < 2 {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "not enough extractable text lines to reconstruct a table".into(),
            });
        }
        report.rows_in = lines.len();

        let header_offset = find_header_offset(&lines);
        if header_offset > 0 {
            report.info(format!(
                "skipped {header_offset} leading line(s) that looked like a title (they broke column alignment)"
            ));
        }
        let table_lines = &lines[header_offset..];

        let spans = infer_column_spans(table_lines);
        if spans.len() < 2 {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "could not infer at least two columns from the extracted text layout".into(),
            });
        }
        report.info(format!(
            "inferred {} column(s) from whitespace alignment in the extracted text (best-effort)",
            spans.len()
        ));

        let headers: Vec<String> = spans.iter().map(|&s| extract_span(table_lines[0], s)).collect();
        let headers: Vec<String> = headers
            .into_iter()
            .enumerate()
            .map(|(i, h)| if h.is_empty() { format!("column_{}", i + 1) } else { h })
            .collect();

        let raw_rows: Vec<Vec<String>> = table_lines[1..].iter().map(|line| spans.iter().map(|&s| extract_span(line, s)).collect()).collect();
        let typed = tidyrs_core::type_columns(&headers, &raw_rows, self.resolver.as_ref());
        for (col, guess, confidence) in &typed.ambiguous_columns {
            report.info(format!(
                "column '{col}': type is ambiguous (best guess: {guess:?}, confidence {confidence:.2}) — kept per-cell inference"
            ));
        }

        let mut table = TidyTable::new(headers).with_source(filename.to_string());
        table.rows = typed.rows;
        table.normalize_row_widths();
        report.rows_out = table.rows.len();

        Ok(ParseOutcome {
            tables: vec![table],
            report,
        })
    }
}
