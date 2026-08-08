//! Excel (.xlsx/.xls) parsing for tidyloom.
//!
//! Handles: merged cells, junk header/footer rows before the real header
//! and after the real data, and multi-sheet workbooks where each sheet is
//! normalized independently (they may have completely different column
//! layouts).
//!
//! Merged cells specifically: calamine exposes *exact* merge-region
//! boundaries (`worksheet_merge_cells`) only on the concrete `Xlsx<RS>`
//! type, not through the generic `Reader` trait used for auto-detected
//! workbooks (`.xls`/`.xlsb`/`.ods` go through a `Sheets` enum that
//! doesn't have that API). So: for real `.xlsx`/`.xlsm` files (detected
//! by their ZIP magic bytes) we open them as `Xlsx<RS>` specifically and
//! fill exactly the cells inside each declared merge region. For every
//! other format we fall back to a column forward-fill heuristic (if a
//! cell is empty, take the last non-empty value above it in the same
//! column) — an approximation, since we have no ground truth for where a
//! merge actually ends in those formats.

use calamine::{open_workbook_auto_from_rs, Data, DataType, Dimensions, Reader, Xlsx};
use std::collections::HashMap;
use std::io::Cursor;
use tidyrs_core::{CleaningReport, ParseOptions, ParseOutcome, TidyError, TidyParser, TidyResult, TidyTable, TidyValue};

pub struct XlsxParser;

impl XlsxParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for XlsxParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts one cell using calamine's *exact*, non-coercive accessors
/// (`get_bool`/`get_int`/`get_float`/`get_string`, each `Some` only for
/// its own matching `Data` variant) rather than `as_i64`/`as_f64`, which
/// silently coerce across types — found via external QA testing to be
/// real, silent data corruption on two different real inputs:
/// - `Data::String("007").as_i64()` returns `Some(7)` (calamine happily
///   `str::parse`s the string), so a column explicitly formatted as Text
///   in the source spreadsheet — postal codes, padded IDs, anything
///   where the leading zero is the whole point — lost that formatting
///   with no warning. `get_string()` keeps the real cell type in view, so
///   the value goes through `TidyValue::infer_from_str` (leading-zero-
///   aware, see `has_meaningful_leading_zero`) instead of calamine's
///   blind coercion.
/// - `Data::Float(v).as_i64()` returns `Some(v as i64)` — an ordinary
///   Rust `as` cast, which *saturates* rather than erroring on overflow.
///   A cell holding `1e300` therefore silently became `i64::MAX`
///   (`9223372036854775807`): not a rounding error, a completely
///   different, wrong number with no signal anything went wrong.
///   `get_float()` only ever matches `Data::Float`, never attempting an
///   int cast at all.
fn cell_to_tidy(cell: &Data) -> TidyValue {
    if cell.is_empty() {
        return TidyValue::Null;
    }
    if let Some(b) = cell.get_bool() {
        return TidyValue::Bool(b);
    }
    // A cell whose number format marks it as a date (checked before the
    // plain-numeric branches below, since a date is stored as an ordinary
    // float serial number under the hood) — reads as a real calendar
    // date/time instead of the raw, meaningless serial ("46027").
    if let Some(dt) = cell.get_datetime().and_then(|d| d.as_datetime()) {
        return TidyValue::Text(dt.to_string());
    }
    if let Some(i) = cell.get_int() {
        return TidyValue::Int(i);
    }
    if let Some(f) = cell.get_float() {
        return TidyValue::Float(f);
    }
    if let Some(s) = cell.get_string() {
        return TidyValue::infer_from_str(s.trim());
    }
    TidyValue::Text(cell.to_string().trim().to_string())
}

fn non_empty_count(row: &[Data]) -> usize {
    row.iter().filter(|c| !c.is_empty()).count()
}

/// Fills every empty cell strictly inside each declared merge region with
/// that region's top-left value. Because this respects exact boundaries
/// (unlike the heuristic fallback), it's safe to run before or after
/// junk-row trimming — it can never leak a value into an unrelated
/// footer row the way an unbounded column forward-fill could.
fn merge_fill_exact(rows: &mut [Vec<Data>], regions: &[Dimensions]) {
    for region in regions {
        let (r0, c0) = region.start;
        let (r1, c1) = region.end;
        let top_left = rows.get(r0 as usize).and_then(|row| row.get(c0 as usize)).cloned().unwrap_or(Data::Empty);
        if top_left.is_empty() {
            continue;
        }
        for r in r0..=r1 {
            let Some(row) = rows.get_mut(r as usize) else { continue };
            for c in c0..=c1 {
                if let Some(cell) = row.get_mut(c as usize) {
                    if cell.is_empty() {
                        *cell = top_left.clone();
                    }
                }
            }
        }
    }
}

/// Column forward-fill fallback for formats where calamine doesn't expose
/// real merge regions: not exact, but a reasonable approximation.
fn merge_fill_heuristic(rows: &mut [Vec<Data>]) {
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    for col in 0..width {
        let mut last: Option<Data> = None;
        for row in rows.iter_mut() {
            if col >= row.len() {
                continue;
            }
            if row[col].is_empty() {
                if let Some(prev) = &last {
                    row[col] = prev.clone();
                }
            } else {
                last = Some(row[col].clone());
            }
        }
    }
}

enum MergeStrategy {
    Exact(Vec<Dimensions>),
    Heuristic,
    None,
}

/// Shared per-sheet processing: junk-row trimming, merge-cell filling,
/// header extraction, and row conversion. Used for both the exact-merge
/// (.xlsx) and heuristic-merge (everything else) code paths so the actual
/// table-shaping logic isn't duplicated.
fn process_sheet(sheet_name: &str, mut rows: Vec<Vec<Data>>, strategy: MergeStrategy, report: &mut CleaningReport) -> Option<TidyTable> {
    if rows.is_empty() {
        report.warning(format!("sheet '{sheet_name}': empty, skipped"));
        return None;
    }

    // Junk-row detection runs on the original emptiness pattern. Exact
    // merge fill is boundary-safe and could run either side of this, but
    // we keep the order consistent with the heuristic path. Exact fill
    // runs first instead: it can only ever populate cells inside a real
    // declared merge region, so it cannot manufacture a false-positive
    // "populated" junk row the way the heuristic could — and running it
    // first is required so that the *last row of a genuine vertical
    // merge* (which has real data but, pre-fill, only one populated
    // cell) doesn't get misread as a single-cell footer and trimmed.
    if let MergeStrategy::Exact(regions) = &strategy {
        if !regions.is_empty() {
            merge_fill_exact(&mut rows, regions);
            report.info(format!(
                "sheet '{sheet_name}': filled {} merged region(s) using exact boundaries",
                regions.len()
            ));
        }
    }

    // A genuinely single-column sheet (a "Notes" tab, a plain list) has
    // at most 1 populated cell in *every* row, including its real data —
    // the ">= 2 populated cells" signal used below to spot a header and
    // trim a footer only makes sense when the sheet has more than one
    // column to begin with. Without this check, a legitimate one-column
    // table used to have its header detection silently fall through
    // (`position` finds nothing, defaults to row 0 — right by luck, not
    // ​design) and then lose every single data row to the footer trim,
    // since a normal data row and a stray footer note are
    // indistinguishable by cell count alone when there's only ever one
    // cell to count.
    let max_populated = rows.iter().map(|r| non_empty_count(r)).max().unwrap_or(0);
    let is_single_column = max_populated <= 1;

    let header_idx = if is_single_column {
        rows.iter().position(|r| non_empty_count(r) >= 1).unwrap_or(0)
    } else {
        rows.iter().position(|r| non_empty_count(r) >= 2).unwrap_or(0)
    };
    if header_idx > 0 {
        report.info(format!("sheet '{sheet_name}': skipped {header_idx} leading junk row(s) before header"));
    }

    let mut end_idx = rows.len();
    if !is_single_column {
        while end_idx > header_idx + 1 {
            let n = non_empty_count(&rows[end_idx - 1]);
            if n <= 1 {
                end_idx -= 1;
            } else {
                break;
            }
        }
    }
    let trimmed_footer = rows.len() - end_idx;
    if trimmed_footer > 0 {
        report.info(format!("sheet '{sheet_name}': trimmed {trimmed_footer} trailing junk/footer row(s)"));
    }

    if matches!(strategy, MergeStrategy::Heuristic) {
        merge_fill_heuristic(&mut rows[header_idx..end_idx]);
        report.info(format!(
            "sheet '{sheet_name}': forward-filled empty cells left by merged regions (heuristic — exact merge boundaries aren't available for this file format)"
        ));
    }

    let header_row = rows[header_idx].clone();
    let width = header_row.len().max(rows[header_idx..end_idx].iter().map(|r| r.len()).max().unwrap_or(0));

    let mut headers: Vec<String> = (0..width)
        .map(|i| {
            let raw = header_row.get(i).map(|c| c.to_string().trim().to_string()).unwrap_or_default();
            if raw.is_empty() {
                format!("column_{}", i + 1)
            } else {
                raw
            }
        })
        .collect();
    let mut seen = HashMap::new();
    for h in headers.iter_mut() {
        let count = seen.entry(h.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            *h = format!("{h}_{count}");
        }
    }

    let mut table = TidyTable::new(headers).with_source(sheet_name.to_string());
    report.rows_in += end_idx.saturating_sub(header_idx + 1);
    for row in &rows[header_idx + 1..end_idx] {
        let values: Vec<TidyValue> = row.iter().map(cell_to_tidy).collect();
        table.push_row(values);
    }
    table.normalize_row_widths();
    report.rows_out += table.rows.len();
    Some(table)
}

impl TidyParser for XlsxParser {
    fn format_name(&self) -> &'static str {
        "xlsx"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let mut score: f32 = 0.0;
        if let Some(name) = filename {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".xlsx") || lower.ends_with(".xlsm") {
                score += 0.4;
            } else if lower.ends_with(".xls") {
                score += 0.3;
            }
        }
        // xlsx/xlsm are zip archives (PK magic); legacy xls is an OLE
        // compound file (D0 CF 11 E0). Either magic is an equally strong
        // signal, hence the same score bump.
        if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) || bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
            score += 0.5;
        }
        score.min(1.0)
    }

    fn parse(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome> {
        // calamine (and the zip/quick-xml crates underneath it) contain
        // internal indexing/`.unwrap()` calls that a corrupted-but-not-
        // rejected .xlsx can hit (confirmed by this crate's proptest
        // robustness suite — a mutated real workbook triggered an
        // "index out of bounds" panic deep in calamine's cell reader
        // rather than returning an Err). We don't control that
        // third-party code, but a corrupt input file must never take
        // down a process meant to run unattended in a pipeline, so the
        // whole parse is isolated behind `catch_unwind`.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.parse_impl(bytes, filename, options))) {
            Ok(result) => result,
            Err(_) => Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "the Excel parsing library panicked on this file (it's likely corrupt or malformed) — \
                          this was caught to avoid crashing the process, but the file could not be parsed"
                    .into(),
            }),
        }
    }
}

impl XlsxParser {
    fn parse_impl(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let mut report = CleaningReport::new(filename, self.format_name());
        let merge_fill = options.get_bool("merge_fill", true);
        let only_sheet = options.get("sheet");

        // Real OOXML (.xlsx/.xlsm) files are ZIP archives: try the
        // concrete Xlsx<RS> reader first so we get exact merge-region
        // boundaries instead of the forward-fill approximation.
        if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
            if let Ok(mut workbook) = Xlsx::new(Cursor::new(bytes)) {
                let sheet_names: Vec<String> = workbook.sheet_names().to_vec();
                if sheet_names.is_empty() {
                    return Err(TidyError::Parse {
                        format: self.format_name().into(),
                        message: "workbook has no sheets".into(),
                    });
                }
                report.info(format!("found {} sheet(s): {}", sheet_names.len(), sheet_names.join(", ")));

                let mut tables = Vec::new();
                for sheet_name in &sheet_names {
                    if let Some(only) = only_sheet {
                        if only != sheet_name {
                            continue;
                        }
                    }
                    let range = match workbook.worksheet_range(sheet_name) {
                        Ok(r) => r,
                        Err(e) => {
                            report.warning(format!("sheet '{sheet_name}': could not read ({e}), skipped"));
                            continue;
                        }
                    };
                    let rows: Vec<Vec<Data>> = range.rows().map(|r| r.to_vec()).collect();

                    let strategy = if merge_fill {
                        match workbook.worksheet_merge_cells(sheet_name) {
                            Some(Ok(regions)) => MergeStrategy::Exact(regions),
                            _ => MergeStrategy::None,
                        }
                    } else {
                        MergeStrategy::None
                    };

                    if let Some(table) = process_sheet(sheet_name, rows, strategy, &mut report) {
                        tables.push(table);
                    }
                }

                if tables.is_empty() {
                    return Err(TidyError::Parse {
                        format: self.format_name().into(),
                        message: "no usable sheets found".into(),
                    });
                }
                return Ok(ParseOutcome { tables, report });
            }
        }

        // Fallback: any other calamine-supported format (.xls, .xlsb,
        // .ods), or an .xlsx that failed the strict Xlsx<RS> open above.
        let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|e| TidyError::Parse {
            format: self.format_name().into(),
            message: e.to_string(),
        })?;

        let sheet_names: Vec<String> = workbook.sheet_names().to_vec();
        if sheet_names.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "workbook has no sheets".into(),
            });
        }
        report.info(format!("found {} sheet(s): {}", sheet_names.len(), sheet_names.join(", ")));

        let mut tables = Vec::new();
        for sheet_name in &sheet_names {
            if let Some(only) = only_sheet {
                if only != sheet_name {
                    continue;
                }
            }
            let range = match workbook.worksheet_range(sheet_name) {
                Ok(r) => r,
                Err(e) => {
                    report.warning(format!("sheet '{sheet_name}': could not read ({e}), skipped"));
                    continue;
                }
            };
            let rows: Vec<Vec<Data>> = range.rows().map(|r| r.to_vec()).collect();
            let strategy = if merge_fill { MergeStrategy::Heuristic } else { MergeStrategy::None };
            if let Some(table) = process_sheet(sheet_name, rows, strategy, &mut report) {
                tables.push(table);
            }
        }

        if tables.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "no usable sheets found".into(),
            });
        }

        Ok(ParseOutcome { tables, report })
    }
}
