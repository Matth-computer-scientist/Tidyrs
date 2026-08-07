//! SQLite database reading for tidyloom — one `TidyTable` per user table in
//! the database, mirroring how `tidyrs-xlsx` produces one table per sheet.
//!
//! Detection here is unusually solid compared to every other format in
//! this workspace: SQLite files start with a fixed, unambiguous 16-byte
//! magic string (`"SQLite format 3\0"`), so there's no heuristic scoring
//! to get wrong the way there is for CSV/fixed-width/YAML/INI content-only
//! detection.
//!
//! SQLite is a paged, random-access on-disk format — there's no supported
//! way to open a connection directly against an in-memory byte slice, so
//! `parse` writes the input to a throwaway temp file first and opens that
//! read-only.

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::io::Write;
use tidyrs_core::{CleaningReport, ParseOptions, ParseOutcome, TidyError, TidyParser, TidyResult, TidyTable, TidyValue};

pub struct SqliteParser;

impl SqliteParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SqliteParser {
    fn default() -> Self {
        Self::new()
    }
}

const MAGIC: &[u8] = b"SQLite format 3\0";

fn has_sqlite_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".db") || lower.ends_with(".sqlite") || lower.ends_with(".sqlite3") || lower.ends_with(".db3")
}

fn sqlite_value_to_tidy(v: ValueRef, blob_columns_seen: &mut usize) -> TidyValue {
    match v {
        ValueRef::Null => TidyValue::Null,
        ValueRef::Integer(i) => TidyValue::Int(i),
        ValueRef::Real(f) => TidyValue::Float(f),
        // SQLite text is already known to be text by the database's own
        // type affinity — unlike CSV/fixed-width, there's no ambiguity to
        // re-infer here, so it's taken as-is rather than run back through
        // TidyValue::infer_from_str.
        ValueRef::Text(t) => TidyValue::Text(String::from_utf8_lossy(t).into_owned()),
        // TidyValue has no binary variant; a blob becomes a descriptive
        // placeholder rather than silently dropping the column or
        // panicking on non-UTF-8 bytes. read_table counts these so the
        // caller gets one summary note instead of one per cell.
        ValueRef::Blob(b) => {
            *blob_columns_seen += 1;
            TidyValue::Text(format!("<blob: {} bytes>", b.len()))
        }
    }
}

fn read_table(conn: &Connection, name: &str, report: &mut CleaningReport, blob_cells_seen: &mut usize) -> rusqlite::Result<Option<TidyTable>> {
    // `name` always comes from sqlite_master's own listing (never
    // arbitrary user/network input), so interpolating it is safe —
    // rusqlite has no bind-parameter placeholder for identifiers, only
    // values.
    let query = format!("SELECT * FROM \"{}\"", name.replace('"', "\"\""));
    let mut stmt = conn.prepare(&query)?;
    let headers: Vec<String> = stmt.column_names().into_iter().map(|s| s.to_string()).collect();
    if headers.is_empty() {
        return Ok(None);
    }

    let mut table = TidyTable::new(headers.clone()).with_source(name.to_string());
    let mut rows_in = 0usize;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        rows_in += 1;
        let mut values = Vec::with_capacity(headers.len());
        for i in 0..headers.len() {
            values.push(sqlite_value_to_tidy(row.get_ref(i)?, blob_cells_seen));
        }
        table.push_row(values);
    }
    table.normalize_row_widths();
    report.rows_in += rows_in;
    report.rows_out += table.rows.len();
    Ok(Some(table))
}

impl TidyParser for SqliteParser {
    fn format_name(&self) -> &'static str {
        "sqlite"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let mut score: f32 = 0.0;
        if let Some(name) = filename {
            if has_sqlite_extension(name) {
                score += 0.2;
            }
        }
        if bytes.starts_with(MAGIC) {
            score += 0.8;
        }
        score.min(1.0)
    }

    fn parse(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome> {
        // rusqlite's own error handling isn't complete for corrupt-but-
        // header-valid databases: `Statement::column_names()` calls
        // `.expect()` internally and panics outright on a non-UTF-8
        // column name rather than returning an `Err` (confirmed by this
        // crate's proptest robustness suite — a single mutated byte deep
        // in a corrupted database's schema was enough to trigger it). The
        // same class of bug already required `catch_unwind` around
        // calamine (tidyrs-xlsx) and pdf-extract (tidyrs-pdf); a corrupt
        // input file must never take the process down here either.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.parse_impl(bytes, filename, options))).unwrap_or_else(|_| {
            Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "the SQLite library panicked on this file (it's likely corrupt) — this was caught to avoid crashing the process, \
                          but the file could not be parsed"
                    .into(),
            })
        })
    }
}

impl SqliteParser {
    fn parse_impl(&self, bytes: &[u8], filename: &str, options: &ParseOptions) -> TidyResult<ParseOutcome> {
        if !bytes.starts_with(MAGIC) {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "not a SQLite database file (missing the 'SQLite format 3' header)".into(),
            });
        }

        let mut tmp = tempfile::Builder::new().suffix(".sqlite").tempfile().map_err(|e| TidyError::Parse {
            format: self.format_name().into(),
            message: format!("could not create a temporary file to open the database: {e}"),
        })?;
        tmp.write_all(bytes).map_err(|e| TidyError::Parse {
            format: self.format_name().into(),
            message: format!("could not write temporary database file: {e}"),
        })?;

        let conn = Connection::open_with_flags(tmp.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| TidyError::Parse {
            format: self.format_name().into(),
            message: format!("could not open database: {e}"),
        })?;

        let mut report = CleaningReport::new(filename, self.format_name());

        let table_names: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .map_err(|e| TidyError::Parse {
                    format: self.format_name().into(),
                    message: format!("could not list tables: {e}"),
                })?;
            let names: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| TidyError::Parse {
                    format: self.format_name().into(),
                    message: format!("could not list tables: {e}"),
                })?
                .filter_map(|r| r.ok())
                .collect();
            names
        };

        if table_names.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "database has no user tables".into(),
            });
        }
        report.info(format!("found {} table(s): {}", table_names.len(), table_names.join(", ")));

        let only_table = options.get("table");
        let mut tables = Vec::new();
        let mut blob_cells_seen = 0usize;
        for name in &table_names {
            if let Some(only) = only_table {
                if only != name {
                    continue;
                }
            }
            match read_table(&conn, name, &mut report, &mut blob_cells_seen) {
                Ok(Some(table)) => tables.push(table),
                Ok(None) => report.warning(format!("table '{name}': no columns, skipped")),
                Err(e) => report.warning(format!("table '{name}': could not read ({e}), skipped")),
            }
        }

        if blob_cells_seen > 0 {
            report.info(format!(
                "{blob_cells_seen} blob cell(s) have no text representation and were replaced with a '<blob: N bytes>' placeholder"
            ));
        }

        if tables.is_empty() {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "no usable tables found".into(),
            });
        }

        Ok(ParseOutcome { tables, report })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_scores_the_magic_header_highly_regardless_of_filename() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&[0u8; 100]);
        let parser = SqliteParser::new();
        assert!(parser.sniff(&bytes, None) > 0.7);
        assert!(parser.sniff(&bytes, Some("data.db")) > 0.9);
    }

    #[test]
    fn sniff_rejects_content_without_the_magic_header_even_with_a_matching_extension() {
        let parser = SqliteParser::new();
        assert!(parser.sniff(b"not a real database", Some("data.db")) < 0.5);
    }

    #[test]
    fn parse_rejects_non_sqlite_bytes_cleanly() {
        let parser = SqliteParser::new();
        let result = parser.parse(b"not a database", "fake.db", &ParseOptions::new());
        assert!(result.is_err());
    }
}
