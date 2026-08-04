//! Chaotic CSV parsing for tidyloom.
//!
//! Handles three classes of real-world mess: unknown delimiter, unknown/
//! non-UTF-8 encoding, and ragged rows (wrong column count) that would
//! otherwise abort a strict CSV parse.

mod stream;

pub use stream::stream_clean_csv;

use tidyrs_core::{
    AmbiguityResolver, CleaningReport, ParseOptions, ParseOutcome, RuleBasedResolver, TidyError, TidyParser, TidyResult,
    TidyTable,
};

const CANDIDATE_DELIMITERS: [u8; 4] = [b',', b';', b'\t', b'|'];

pub struct CsvParser {
    resolver: Box<dyn AmbiguityResolver>,
}

impl CsvParser {
    pub fn new() -> Self {
        Self { resolver: Box::new(RuleBasedResolver) }
    }

    /// Swaps in a different [`AmbiguityResolver`] for column-type
    /// inference — e.g. `HttpLlmResolver` (behind tidyrs-core's `llm`
    /// feature) for cases the default rule-based resolver can't settle
    /// confidently.
    pub fn with_resolver(resolver: Box<dyn AmbiguityResolver>) -> Self {
        Self { resolver }
    }
}

impl Default for CsvParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Decodes `bytes` to a UTF-8 `String`, detecting the source encoding when
/// it isn't already valid UTF-8. Returns the decoded text and a label for
/// the encoding that was used, for reporting.
pub(crate) fn decode_bytes(bytes: &[u8]) -> (String, &'static str) {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_string(), "UTF-8");
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let encoding = detector.guess(None, true);
    let (decoded, _, _) = encoding.decode(bytes);
    (decoded.into_owned(), encoding.name())
}

/// Counts occurrences of `delim` in `line`, ignoring anything between a
/// pair of double quotes (a quoted field like `"loves, markets"` must not
/// make the delimiter-sniffer think there are extra commas separating
/// columns). This is a simple toggle, not a full CSV-quoting state
/// machine (it doesn't handle escaped `""` inside a quoted field), which
/// is intentionally enough for sniffing — the real parse below uses the
/// `csv` crate's proper quote handling regardless.
fn count_delimiter_outside_quotes(line: &str, delim: u8) -> usize {
    let mut in_quotes = false;
    let mut count = 0;
    for b in line.bytes() {
        match b {
            b'"' => in_quotes = !in_quotes,
            b if b == delim && !in_quotes => count += 1,
            _ => {}
        }
    }
    count
}

/// Picks the delimiter whose per-line occurrence count is the most
/// consistent (lowest variance) across a sample of non-empty lines, among
/// comma / semicolon / tab / pipe. Falls back to comma if the file is too
/// short/uniform to tell.
pub(crate) fn detect_delimiter(text: &str) -> u8 {
    let sample: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(30).collect();
    if sample.is_empty() {
        return b',';
    }

    let mut best = b',';
    let mut best_score = f64::MIN;

    for &delim in &CANDIDATE_DELIMITERS {
        let counts: Vec<usize> = sample
            .iter()
            .map(|line| count_delimiter_outside_quotes(line, delim))
            .collect();
        let total: usize = counts.iter().sum();
        if total == 0 {
            continue;
        }
        let mean = total as f64 / counts.len() as f64;
        if mean < 0.5 {
            continue;
        }
        let variance = counts.iter().map(|&c| (c as f64 - mean).powi(2)).sum::<f64>() / counts.len() as f64;
        // Reward high, stable counts: high mean, low variance.
        let score = mean - variance;
        if score > best_score {
            best_score = score;
            best = delim;
        }
    }
    best
}

impl TidyParser for CsvParser {
    fn format_name(&self) -> &'static str {
        "csv"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let mut score: f32 = 0.0;
        if let Some(name) = filename {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".csv") || lower.ends_with(".tsv") {
                score += 0.5;
            }
        }
        let (text, _) = decode_bytes(&bytes[..bytes.len().min(4096)]);
        let delim = detect_delimiter(&text);
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(10).collect();
        if lines.len() >= 2 {
            let counts: Vec<usize> = lines.iter().map(|l| count_delimiter_outside_quotes(l, delim)).collect();
            if counts.iter().all(|&c| c == counts[0]) && counts[0] > 0 {
                score += 0.4;
            }
        }
        score.min(1.0)
    }

    fn parse(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let mut report = CleaningReport::new(filename, self.format_name());

        let (text, encoding_used) = decode_bytes(bytes);
        if encoding_used != "UTF-8" {
            report.warning(format!(
                "input was not valid UTF-8; decoded using detected encoding {encoding_used}"
            ));
        }

        let delimiter = match options.get("delimiter") {
            Some(d) if d.len() == 1 => d.as_bytes()[0],
            Some(other) => {
                report.warning(format!("ignoring invalid --delimiter '{other}', auto-detecting instead"));
                detect_delimiter(&text)
            }
            None => detect_delimiter(&text),
        };
        report.info(format!("detected delimiter: {:?}", delimiter as char));

        let has_header = options.get_bool("has_header", true);

        let mut rdr = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .flexible(true)
            .has_headers(false)
            .from_reader(text.as_bytes());

        let mut all_rows: Vec<csv::StringRecord> = Vec::new();
        for result in rdr.records() {
            match result {
                Ok(record) => all_rows.push(record),
                Err(e) => {
                    report.warning(format!("skipped unparseable line: {e}"));
                }
            }
        }

        if all_rows.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "no rows found".into(),
            });
        }

        report.rows_in = all_rows.len();

        let expected_width = all_rows
            .iter()
            .map(|r| r.len())
            .fold(std::collections::HashMap::<usize, usize>::new(), |mut acc, len| {
                *acc.entry(len).or_insert(0) += 1;
                acc
            })
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(len, _)| len)
            .unwrap_or(0);

        let headers: Vec<String> = if has_header {
            all_rows[0].iter().map(|s| s.trim().to_string()).collect()
        } else {
            (0..expected_width).map(|i| format!("column_{}", i + 1)).collect()
        };
        let mut headers = headers;
        if headers.len() < expected_width {
            for i in headers.len()..expected_width {
                headers.push(format!("column_{}", i + 1));
            }
        }

        let data_rows = if has_header { &all_rows[1..] } else { &all_rows[..] };
        let mut malformed = 0usize;
        let mut raw_rows: Vec<Vec<String>> = Vec::with_capacity(data_rows.len());
        for record in data_rows {
            if record.len() != expected_width {
                malformed += 1;
            }
            let mut row: Vec<String> = record.iter().map(|s| s.to_string()).collect();
            row.resize(expected_width, String::new());
            row.truncate(expected_width);
            raw_rows.push(row);
        }

        if malformed > 0 {
            report.warning(format!(
                "{malformed} row(s) had an inconsistent column count and were padded/truncated to {expected_width} columns"
            ));
        }

        // Column-wide typing via the AmbiguityResolver extension point,
        // instead of inferring each cell's type independently — see
        // tidyrs_core::typing for why that distinction matters.
        let typed = tidyrs_core::type_columns(&headers, &raw_rows, self.resolver.as_ref());
        for (col, guess, confidence) in &typed.ambiguous_columns {
            report.info(format!(
                "column '{col}': type is ambiguous (best guess: {guess:?}, confidence {confidence:.2}) — kept per-cell inference; \
                 a stronger AmbiguityResolver (e.g. HttpLlmResolver) may resolve this more consistently"
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
