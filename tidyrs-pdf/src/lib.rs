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
//!
//! One specific "visually complex" case worth calling out precisely,
//! found via external QA testing: a page that mixes a real table with a
//! separate free-text paragraph (e.g. a "Comments:" block below the
//! data) does not read as garbled or truncated text — [`glyphs`]'s
//! extraction is accurate down to the character; verified by dumping its
//! raw output directly. The problem is one layer up: [`infer_column_spans`]
//! looks for whitespace alignment across *every* remaining line
//! uniformly, with no concept of "the table ends here, free text starts."
//! A paragraph's word-wrapped lines don't share the table's column
//! structure at all, so whatever weak, coincidental alignment survives
//! across the mixed set gets used as real column boundaries — cutting
//! paragraph sentences at whatever position happens to land there rather
//! than at word boundaries. Fixing this properly needs a real "where does
//! the tabular region end" detector (the same class of problem
//! `tidyrs-xlsx`'s footer-trimming solves for spreadsheets, which this
//! crate's own docs already admit it doesn't have an equivalent of) —
//! out of scope for a targeted fix; noted here as a precisely diagnosed,
//! not just vaguely acknowledged, limitation.
//!
//! A follow-up investigation into this same case found something worth
//! fixing on its own, narrower than the "table end" detector above: a
//! prose character can land exactly on a "gap" column position that every
//! real table row leaves blank (pure word-wrap coincidence), and the row-
//! extraction used to map each inferred column span over the line
//! independently, with no way to preserve a character that fell *between*
//! spans — so it was silently dropped rather than merely misplaced (e.g.
//! "regions" losing its leading "r" entirely, not landing in the wrong
//! cell). [`extract_row`] fixes exactly that: a gap-position character is
//! now glued onto the nearest cell instead of discarded. The paragraph
//! still doesn't reconstruct correctly — that's still the same
//! out-of-scope "table end" problem — but no character is silently lost
//! doing it, which is the actual guarantee this crate promises elsewhere.
//! This was safe to fix outright (unlike the `find_header_offset`
//! counter-examples below) because it's a no-op on any well-aligned
//! table: a gap position is blank on nearly every row by definition, so
//! there's essentially never real content there to preserve.

mod glyphs;

use tidyrs_core::{AmbiguityResolver, CleaningReport, ParseOptions, ParseOutcome, RuleBasedResolver, TidyError, TidyParser, TidyResult, TidyTable};

pub struct PdfParser {
    resolver: Box<dyn AmbiguityResolver>,
}

impl PdfParser {
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

impl Default for PdfParser {
    fn default() -> Self {
        Self::new()
    }
}

/// A character position counts as a column gap if it's whitespace on at
/// least this fraction of rows. Requiring *every* row to agree used to
/// mean a single overflowing cell — a long name spilling past its column,
/// a right-aligned number reaching one character further left than its
/// neighbors — permanently glued two real columns into one for the whole
/// table, since that one row's non-whitespace character at the gap
/// position was enough to veto it. Real tables tolerate the occasional
/// outlier row; this mirrors that instead of demanding pixel-perfect
/// alignment from every single line.
const GAP_AGREEMENT_THRESHOLD: f64 = 0.85;

fn infer_column_spans(lines: &[&str]) -> Vec<(usize, usize)> {
    let max_len = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    if max_len == 0 || lines.is_empty() {
        return vec![];
    }
    let chars: Vec<Vec<char>> = lines.iter().map(|l| l.chars().collect()).collect();

    let mut whitespace_count = vec![0usize; max_len];
    for row in &chars {
        for (pos, count) in whitespace_count.iter_mut().enumerate() {
            let c = row.get(pos).copied().unwrap_or(' ');
            if c.is_whitespace() {
                *count += 1;
            }
        }
    }
    let is_gap: Vec<bool> = whitespace_count
        .iter()
        .map(|&count| count as f64 / lines.len() as f64 >= GAP_AGREEMENT_THRESHOLD)
        .collect();

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

/// Splits one line into `spans.len()` cells, one per span (the characters
/// in `span.0..span.1`, trimmed) — except a non-whitespace character that
/// lands *between* two spans (a real "gap" position, by definition blank
/// on the overwhelming majority of rows — see [`GAP_AGREEMENT_THRESHOLD`])
/// is appended to the nearest preceding cell instead of silently
/// discarded.
///
/// Found via external QA testing: a page mixing a real table with a
/// free-text paragraph (see the module docs) can, on the paragraph's
/// lines, have real prose characters fall exactly on a gap position that
/// every table row leaves blank — a coincidence of word-wrap, not
/// evidence the paragraph shares the table's structure. An earlier
/// version of this function mapped each span independently over the
/// line, which has no way to preserve characters that fall in the space
/// *between* spans, so they were dropped outright — genuine, silent
/// character loss (e.g. "regions" losing its leading "r"), not just
/// characters landing in the "wrong" column. Gluing them onto the nearest
/// cell doesn't reconstruct the paragraph correctly (that still needs the
/// "where does the table end" detector the module docs describe as out
/// of scope), but it upholds this crate's actual guarantee: garbled
/// structure is reviewable, silently missing data is not. On a real,
/// well-aligned table this is a no-op — a gap position is, by
/// construction, blank on nearly every row, so there's essentially never
/// non-whitespace content to glue anywhere.
fn extract_row(line: &str, spans: &[(usize, usize)]) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut cells = vec![String::new(); spans.len()];
    for (pos, &ch) in chars.iter().enumerate() {
        if let Some(i) = spans.iter().position(|&(s, e)| pos >= s && pos < e) {
            cells[i].push(ch);
            continue;
        }
        if ch.is_whitespace() {
            continue;
        }
        // Real content in a gap position: attach to the last span that
        // ends at or before this position, or the first span if this
        // falls before every span (e.g. an unexpectedly long line with
        // its own leading indent).
        let target = spans.iter().rposition(|&(_, e)| e <= pos).unwrap_or(0);
        cells[target].push(ch);
    }
    cells.into_iter().map(|s| s.trim().to_string()).collect()
}

/// A title line above the real header (e.g. "Quarterly Sales Report")
/// usually breaks whitespace alignment with the rest of the table: its
/// text doesn't line up with the columns below it, so including it in
/// `infer_column_spans` merges what should be separate columns into one
/// wide span. We don't have a header/footer detector as principled as
/// `tidyrs-xlsx`'s (there's no "populated cell count" concept in raw
/// text), so instead we try dropping 0, 1, 2, ... leading lines and keep
/// whichever skip count *first* reaches the maximum number of inferred
/// columns — a title line disrupting alignment should make the column
/// count go up once it's excluded, and further skips are wasted once it
/// plateaus.
///
/// That last part matters and used to be broken: comparing with a plain
/// running "> best" let the loop keep chasing any *later* skip that also
/// happened to increase the span count, even by accident — e.g. skipping
/// the real header line too (on top of the title) can occasionally look
/// like an "improvement" once column spacing is measured precisely
/// (real glyph positions rather than character counts), because a
/// shorter header row's rounding-derived alignment doesn't always land
/// in exactly the same character cells as the data rows below it. That
/// swallowed the header itself as if it were more junk. Finding the true
/// maximum first and keeping the *smallest* skip that reaches it (not
/// just any skip that ties or exceeds a running value) is what the
/// docstring already promised and avoids that over-skip.
///
/// A tempting further refinement — among skip levels that tie on span
/// *count*, prefer the one with the smallest total span *width* on the
/// theory that a title line merges columns into wider spans — was tried
/// and reverted: it isn't a sound general signal (different row subsets
/// legitimately produce different span boundaries for reasons unrelated
/// to junk-line pollution) and caused real regressions, over-skipping
/// genuine header rows in files that had no title line at all. A file
/// whose title-excluded and header-excluded span counts happen to tie
/// exactly (only observed so far with a monospaced font, where character
/// counting and geometric spacing coincide) can still lose its header to
/// this heuristic — a known, narrow remaining limitation rather than one
/// worth another heuristic layer.
///
/// A second, more serious way the search could over-skip was found via
/// external QA testing on a table with a title *and* a couple of ragged
/// data rows (some cells legitimately empty): `infer_column_spans`'s
/// whitespace-agreement threshold is a *fraction of however many rows are
/// in the slice being scored* — which means skipping more leading lines
/// doesn't just remove candidate title lines, it also shrinks the sample
/// the 85% agreement bar is measured against, making that bar easier to
/// clear on fewer, unrelated grounds. On the reported file this let
/// `skip=3` (discarding the title, the real header, *and* a real data
/// row) score higher than `skip=1` (discarding only the title) — losing
/// a whole data row and misreading a data row as the header, not just a
/// cosmetic misalignment. Capping how many lines this search is even
/// allowed to try skipping (see `MAX_TITLE_SKIP`) bounds the damage: a
/// single-line title is the overwhelming common case, so there's no
/// realistic upside to letting the search reach past that far enough to
/// start gaming its own scoring function on real data rows.
///
/// A related counter-example (same font, no ragged data this time) was
/// found in a follow-up report: a multi-word title's own internal word
/// gaps ("Rapport" / "Ventes" / "-" / "Janvier 2026") can coincidentally
/// subdivide a region the real table only ever sees as *one* wide gap —
/// making `skip=0` (title included) score *more* columns than `skip=1`
/// (title excluded), the opposite of what this search assumes. A more
/// direct replacement — computing the table's columns from every line
/// except the first, then asking directly whether that first line's own
/// content falls inside two or more of them — was prototyped and
/// rejected: it can't tell a title (unrelated prose landing in several
/// column positions) apart from a *genuine header* (whose whole point is
/// to put a label in every column), so it started misreading real
/// headers as titles across multiple existing, previously-passing
/// fixtures. Left as a known limitation for the same reason the tied-
/// count case above is: every fix attempted for it broke a more common
/// case than the one it fixed. `title_with_no_ragged_data.pdf` pins down
/// that the failure stays *bounded* even here — the title survives as
/// extra ambiguous columns, not silently dropped data (see
/// `a_title_the_heuristic_cannot_detect_still_does_not_lose_data` in
/// tests/fixtures.rs).
///
/// A third counter-example, found via a later external QA report, runs in
/// the *opposite* direction from the two above: there, a junk line
/// scoring more columns when *included* fooled the search. Here, on
/// `right_aligned_numbers.pdf`, a genuine header ("product qty amount")
/// scores *fewer* columns than excluding it, because the data rows below
/// it ("Widget A", "Widget B", "Widget C") share an internal-space
/// whitespace gap at one character position that the header text doesn't
/// share — so dropping the header lets that coincidental gap register as
/// a real column boundary, and "more columns wins" picks skip=1 over the
/// correct skip=0. The header is lost outright (not just merged into
/// extra columns, worse than the two cases above) and the first data row
/// is misread as the header in its place. Same underlying flaw as
/// always: total column count isn't a safe proxy for "found the real
/// table" in either direction. Left as a known, bounded limitation for
/// the same reason as the other two — see
/// `a_data_cell_that_looks_like_two_words_can_still_cost_the_header` in
/// tests/fixtures.rs, which pins down that every actual data value still
/// survives even though the header and one row's column split don't.
/// How many leading lines the search below is allowed to try skipping.
/// Deliberately just 1, not "as many as help the score" — see the
/// docstring on `find_header_offset` for why letting this search deeper
/// used to be able to eat real data rows, not just a title line. A
/// genuine title block is overwhelmingly a single line in practice; a
/// multi-line title is a known, accepted limitation, same tier as the
/// multi-line-cell/rotated-text limitations already documented at the
/// module level.
const MAX_TITLE_SKIP: usize = 1;

fn find_header_offset(lines: &[&str]) -> usize {
    let max_skip = lines.len().saturating_sub(2).min(MAX_TITLE_SKIP);
    let span_counts: Vec<usize> = (0..=max_skip).map(|skip| infer_column_spans(&lines[skip..]).len()).collect();
    let best_span_count = span_counts.iter().copied().max().unwrap_or(0);
    span_counts.iter().position(|&c| c == best_span_count).unwrap_or(0)
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

        let headers: Vec<String> = extract_row(table_lines[0], &spans);
        let mut headers: Vec<String> = headers
            .into_iter()
            .enumerate()
            .map(|(i, h)| if h.is_empty() { format!("column_{}", i + 1) } else { h })
            .collect();
        // A repeated header text (two columns both literally titled
        // "Total", for instance) would otherwise produce an ambiguous
        // output header — same disambiguation every other parser in the
        // workspace applies to its own header row.
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for h in headers.iter_mut() {
            let count = seen.entry(h.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                *h = format!("{h}_{count}");
            }
        }

        let raw_rows: Vec<Vec<String>> = table_lines[1..].iter().map(|line| extract_row(line, &spans)).collect();
        report.rows_in = raw_rows.len();
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

        Ok(ParseOutcome { tables: vec![table], report })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_overflowing_row_no_longer_merges_two_columns() {
        // "Christopherson" overflows one character into what is, on every
        // other row, empty column-separator space. Requiring *all* rows to
        // agree on a gap used to let that single outlier veto the column
        // boundary for the entire table.
        let lines = vec!["name           age", "Alice          30", "Christopherson 41", "Bob            22"];
        let spans = infer_column_spans(&lines);
        assert_eq!(spans.len(), 2, "expected 2 columns, got spans {spans:?}");
    }

    #[test]
    fn a_clean_aligned_table_still_splits_on_every_gap() {
        let lines = vec!["name   age  city", "Alice  30   Paris", "Bob    22   Lyon"];
        let spans = infer_column_spans(&lines);
        assert_eq!(spans.len(), 3, "expected 3 columns, got spans {spans:?}");
    }

    #[test]
    fn majority_misaligned_column_is_not_falsely_merged() {
        // If most rows genuinely have content at a position (not just one
        // outlier), it must stay merged rather than being forced apart —
        // the tolerance is for rare overflow, not a license to over-split.
        let lines = vec!["ab cd", "abxcd", "abycd", "ab cd"];
        let spans = infer_column_spans(&lines);
        assert_eq!(spans.len(), 1, "expected the columns to stay merged, got spans {spans:?}");
    }

    #[test]
    fn empty_input_infers_no_spans() {
        assert_eq!(infer_column_spans(&[]), vec![]);
    }
}
