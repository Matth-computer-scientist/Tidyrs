//! Bounded-memory CSV cleaning: reads and writes in a single streaming
//! pass instead of materializing the whole file as a `TidyTable`. This is
//! a separate entry point from [`crate::CsvParser`] (which the
//! `TidyParser` trait requires to hand back an in-memory table) — CSV is
//! the one format where "just don't buffer the whole thing" is both
//! straightforward and actually matters at scale, and the streaming
//! `csv` crate API is doing the same read line-by-line either way, so
//! this comes at a very low complexity cost.
//!
//! Trade-offs versus the in-memory path, both documented deliberately
//! rather than silently accepted:
//! - **Encoding**: only genuinely streams for already-UTF-8 input.
//!   Detecting an arbitrary source encoding needs to see representative
//!   content first; rather than half-stream a decode, non-UTF-8 input
//!   falls back to reading the whole file into memory and reusing
//!   [`crate::CsvParser`] — the same memory profile as before, just for
//!   the (less common) case where the fully-streaming path can't apply.
//! - **Column width**: the in-memory parser picks the *statistically most
//!   common* row width across the whole file as the canonical column
//!   count, which requires having seen every row first. Streaming instead
//!   uses the header row's width (or the first data row's, if there's no
//!   header) — a file whose ragged rows are so numerous that the header
//!   width is a minority shape will normalize differently between the two
//!   paths. This only matters for pathological inputs; typical ragged-row
//!   cases (a handful of short/long rows) behave the same either way.
//! - **Output**: CSV only. JSON needs a full in-memory array to serialize
//!   correctly-shaped objects, and Parquet needs whole columns to infer a
//!   per-column type ([`tidyrs_core::export::write_parquet_file`]) — both
//!   are architecturally incompatible with a single forward pass, so
//!   there's no streaming equivalent for them.
//! - **Number formatting**: streaming writes every field's original text
//!   straight through, unchanged — it never parses a cell into
//!   `TidyValue::Int`/`Float` and re-serializes it the way the in-memory
//!   path does. On real (not uniformly-formatted) data this is visible:
//!   an amount written as `"3756.90"` in the source file stays exactly
//!   `"3756.90"` when streamed, but comes out as `"3756.9"` from the
//!   in-memory path (parsed as an `f64`, trailing zero dropped by
//!   `Display`). Confirmed with a realistic mixed-formatting fixture in
//!   `tidyrs-cli/tests/real_world_scenarios.rs` — don't assume the two
//!   paths are byte-for-byte interchangeable on a column whose source
//!   values don't already share one consistent decimal-place format.

use crate::{decode_bytes, detect_delimiter};
use std::io::{Read, Write};
use tidyrs_core::{CleaningReport, ParseOptions, TidyParser, TidyResult};

const SNIFF_PREFIX_LEN: usize = 65536;

/// Reads up to `buf.len()` bytes, looping on short reads, stopping at EOF.
/// Unlike `Read::read`, a short read here is not assumed to mean EOF.
fn fill_prefix<R: Read>(input: &mut R, buf: &mut Vec<u8>) -> std::io::Result<()> {
    buf.resize(SNIFF_PREFIX_LEN, 0);
    let mut filled = 0;
    loop {
        match input.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
        if filled == buf.len() {
            break;
        }
    }
    buf.truncate(filled);
    Ok(())
}

/// Cleans CSV from `input` to `output` in a single streaming pass, never
/// holding more than [`SNIFF_PREFIX_LEN`] bytes plus one row in memory at
/// a time (for the common UTF-8 case — see module docs for the non-UTF-8
/// fallback). Returns the same [`CleaningReport`] shape as the in-memory
/// path.
pub fn stream_clean_csv<R: Read, W: Write>(mut input: R, output: W, filename: &str, options: &ParseOptions) -> TidyResult<CleaningReport> {
    let mut prefix = Vec::new();
    fill_prefix(&mut input, &mut prefix)?;
    // A BOM is valid UTF-8 (decodes to U+FEFF) so it wouldn't fail the
    // is_err() check below and would otherwise glue itself onto the first
    // header name — same fix as the in-memory path's decode_bytes.
    if tidyrs_core::strip_utf8_bom(&prefix).len() != prefix.len() {
        prefix = tidyrs_core::strip_utf8_bom(&prefix).to_vec();
    }

    if std::str::from_utf8(&prefix).is_err() {
        return stream_clean_csv_non_utf8_fallback(prefix, input, output, filename, options);
    }
    let prefix_text = String::from_utf8(prefix.clone()).expect("just validated as UTF-8 above");

    let mut report = CleaningReport::new(filename, "csv");

    let delimiter = match options.get("delimiter") {
        Some(d) if d.len() == 1 => d.as_bytes()[0],
        Some(other) => {
            report.warning(format!("ignoring invalid --delimiter '{other}', auto-detecting instead"));
            detect_delimiter(&prefix_text)
        }
        None => detect_delimiter(&prefix_text),
    };
    report.info(format!("detected delimiter: {:?}", delimiter as char));
    report.info("streaming mode: bounded memory, single pass".to_string());

    let has_header = options.get_bool("has_header", true);
    let chained = std::io::Cursor::new(prefix).chain(input);

    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .has_headers(false)
        .from_reader(chained);
    let mut wtr = csv::Writer::from_writer(output);

    let mut expected_width: Option<usize> = None;
    let mut rows_in = 0usize;
    let mut rows_out = 0usize;
    let mut malformed = 0usize;
    let mut is_first_record = true;

    for result in rdr.records() {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                report.warning(format!("skipped unparseable line: {e}"));
                continue;
            }
        };

        if is_first_record && has_header {
            expected_width = Some(record.len());
            let headers: Vec<String> = record.iter().map(|s| s.trim().to_string()).collect();
            wtr.write_record(&headers)?;
            is_first_record = false;
            continue;
        }
        is_first_record = false;

        let width = *expected_width.get_or_insert(record.len());
        rows_in += 1;
        if record.len() != width {
            malformed += 1;
        }

        let mut out_record: Vec<String> = record.iter().map(|s| s.to_string()).collect();
        out_record.resize(width, String::new());
        out_record.truncate(width);
        wtr.write_record(&out_record)?;
        rows_out += 1;
    }
    wtr.flush()?;

    report.rows_in = rows_in;
    report.rows_out = rows_out;
    if malformed > 0 {
        report.warning(format!(
            "{malformed} row(s) had an inconsistent column count and were padded/truncated to {} columns",
            expected_width.unwrap_or(0)
        ));
    }
    Ok(report)
}

fn stream_clean_csv_non_utf8_fallback<R: Read, W: Write>(
    prefix: Vec<u8>,
    mut rest: R,
    output: W,
    filename: &str,
    options: &ParseOptions,
) -> TidyResult<CleaningReport> {
    let mut bytes = prefix;
    rest.read_to_end(&mut bytes)?;
    let (text, encoding_used) = decode_bytes(&bytes);

    let parser = crate::CsvParser::new();
    let outcome = parser.parse(text.as_bytes(), filename, options)?;
    let mut report = outcome.report;
    report.warning(format!(
        "input was not valid UTF-8 (decoded using detected encoding {encoding_used}); streaming mode fell back to \
         reading the whole file into memory, since encoding detection needs to see representative content first"
    ));
    tidyrs_core::export::write_csv(&outcome.tables[0], output)?;
    Ok(report)
}
