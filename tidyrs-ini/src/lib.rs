//! INI / `.env` key-value config parsing for tidyloom.
//!
//! Two real-world shapes, both handled by the same grammar (`key = value`
//! lines, `[section]` headers, `#`/`;` full-line *and* trailing comments
//! (see [`strip_trailing_comment`]), an optional `export ` prefix on any
//! line for `.env` files that source directly into a shell):
//!
//! - **Sectioned** (a classic `.ini`, or a multi-profile file like an AWS
//!   `credentials` file with `[default]`/`[work]` blocks): each section
//!   becomes one row, with a `section` column plus one column per key seen
//!   in *any* section — genuinely tabular data, not a single flat record.
//! - **Flat** (a typical `.env`, or an `.ini` with no `[section]` headers
//!   at all): the whole file is one record, one row.
//!
//! Unlike JSON/XML/YAML this grammar has no arbitrary nesting to flatten,
//! so there's no separate flattening pass to document — what you see in
//! the file is what you get as columns.

use std::collections::BTreeSet;
use tidyrs_core::{AmbiguityResolver, CleaningReport, ParseOptions, ParseOutcome, RuleBasedResolver, TidyError, TidyParser, TidyResult, TidyTable};

pub struct IniParser {
    resolver: Box<dyn AmbiguityResolver>,
}

impl IniParser {
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

impl Default for IniParser {
    fn default() -> Self {
        Self::new()
    }
}

fn has_ini_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".ini") || lower.ends_with(".cfg") || lower.ends_with(".conf") || lower.ends_with(".properties")
}

fn has_env_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".env") || lower.rsplit('/').next().is_some_and(|f| f.starts_with(".env"))
}

/// A conservative definition of "looks like a key": plain identifier-ish
/// characters only. This is what keeps the content-only sniff from firing
/// on things that happen to contain a bare `=` (a URL query string, a
/// shell one-liner) — a real config key is never going to contain a `/`
/// or `?`, so requiring this is a cheap, effective filter.
fn looks_like_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

fn is_section_line(line: &str) -> bool {
    line.len() >= 2 && line.starts_with('[') && line.ends_with(']')
}

fn is_kv_line(line: &str) -> bool {
    let content = line.strip_prefix("export ").unwrap_or(line);
    match content.find('=') {
        Some(idx) => looks_like_key(content[..idx].trim()),
        None => false,
    }
}

/// Same two-signal shape as the YAML content sniff in `tidyrs-json`:
/// require most sampled non-comment lines to match the `[section]` /
/// `key=value` grammar, not just "the file contains an `=` somewhere."
fn looks_like_ini_content(text: &str) -> bool {
    let lines = tidyrs_core::representative_lines(text, 20);
    let candidates: Vec<&str> = lines
        .into_iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with(';'))
        .collect();
    if candidates.len() < 2 {
        return false;
    }
    let matching = candidates.iter().filter(|l| is_section_line(l) || is_kv_line(l)).count();
    matching as f32 / candidates.len() as f32 >= 0.8
}

struct Bucket {
    name: String,
    pairs: Vec<(String, String)>,
}

/// Strips one layer of matching `"..."` or `'...'` quoting from a value —
/// the common `.env` convention for values containing spaces or `#`.
fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if value.len() >= 2 && ((bytes[0] == b'"' && bytes[value.len() - 1] == b'"') || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')) {
        return value[1..value.len() - 1].to_string();
    }
    value.to_string()
}

/// Strips a trailing `; comment` or `# comment` from a raw value — the
/// classic INI convention (found missing via external QA testing: it
/// used to be supported nowhere, so a comment marker just became part of
/// the value). A marker only ends the value when it's preceded by
/// whitespace *and* isn't inside a quoted string — `key=http://x#frag`
/// stays whole (no preceding whitespace before `#`), and
/// `key="a; b" ; real comment` only strips the real trailing one, not the
/// `;` inside the quotes. This is a plain scan, not a parser — same
/// "conservative, don't guess" bias `is_kv_line` already uses elsewhere
/// in this crate.
fn strip_trailing_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    let mut in_quotes: Option<u8> = None;
    let mut prev_was_space = false;
    for (i, &b) in bytes.iter().enumerate() {
        match in_quotes {
            Some(q) if b == q => in_quotes = None,
            Some(_) => {}
            None => {
                if b == b'"' || b == b'\'' {
                    in_quotes = Some(b);
                } else if (b == b';' || b == b'#') && prev_was_space {
                    return value[..i].trim_end();
                }
            }
        }
        prev_was_space = b == b' ' || b == b'\t';
    }
    value
}

impl TidyParser for IniParser {
    fn format_name(&self) -> &'static str {
        "ini"
    }

    fn sniff(&self, bytes: &[u8], filename: Option<&str>) -> f32 {
        let has_extension_hint = filename.is_some_and(|n| has_ini_extension(n) || has_env_extension(n));

        let sample = tidyrs_core::sample_for_sniffing(bytes);
        let text = String::from_utf8_lossy(&sample);
        let total_chars = text.chars().count();
        if total_chars == 0 {
            return if has_extension_hint { 0.6 } else { 0.0 };
        }
        // Same binary-content guard as tidyrs-csv/tidyrs-fixed: random
        // bytes decoded lossily can still coincidentally contain a `=` or
        // two, so reject anything that isn't overwhelmingly printable text
        // before trusting the grammar-shape check below.
        let junk_chars = text.chars().filter(|&c| c.is_control() && c != '\n' && c != '\r' && c != '\t').count();
        if junk_chars as f32 / total_chars as f32 > 0.01 {
            return 0.0;
        }

        let content_matches = looks_like_ini_content(&text);
        match (has_extension_hint, content_matches) {
            (true, _) => 0.65,
            (false, true) => 0.55,
            (false, false) => 0.0,
        }
    }

    fn parse(&self, bytes: &[u8], filename: &str, _options: &ParseOptions) -> TidyResult<ParseOutcome> {
        let text = String::from_utf8_lossy(tidyrs_core::strip_utf8_bom(bytes)).into_owned();

        let mut buckets: Vec<Bucket> = Vec::new();
        let mut index_of: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut current_section = String::new();
        let mut has_explicit_section = false;
        let mut malformed = 0usize;
        let mut duplicate_keys = 0usize;

        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if is_section_line(line) {
                let name = line[1..line.len() - 1].trim().to_string();
                if !index_of.contains_key(&name) {
                    index_of.insert(name.clone(), buckets.len());
                    buckets.push(Bucket {
                        name: name.clone(),
                        pairs: Vec::new(),
                    });
                }
                current_section = name;
                has_explicit_section = true;
                continue;
            }

            let content = line.strip_prefix("export ").unwrap_or(line);
            let Some(eq_idx) = content.find('=') else {
                malformed += 1;
                continue;
            };
            let key = content[..eq_idx].trim().to_string();
            let value = unquote(strip_trailing_comment(content[eq_idx + 1..].trim()));
            if key.is_empty() {
                malformed += 1;
                continue;
            }

            let idx = *index_of.entry(current_section.clone()).or_insert_with(|| {
                buckets.push(Bucket {
                    name: current_section.clone(),
                    pairs: Vec::new(),
                });
                buckets.len() - 1
            });
            if let Some(existing) = buckets[idx].pairs.iter_mut().find(|(k, _)| *k == key) {
                existing.1 = value;
                duplicate_keys += 1;
            } else {
                buckets[idx].pairs.push((key, value));
            }
        }

        if buckets.is_empty() || buckets.iter().all(|b| b.pairs.is_empty()) {
            return Err(TidyError::Parse {
                format: self.format_name().into(),
                message: "no key=value pairs found".into(),
            });
        }

        let format_label = if has_env_extension(filename) { "env" } else { "ini" };
        let mut report = CleaningReport::new(filename, format_label);
        if malformed > 0 {
            report.warning(format!(
                "{malformed} line(s) were neither a [section] header, a comment, nor a valid key=value pair and were skipped"
            ));
        }
        if duplicate_keys > 0 {
            report.warning(format!(
                "{duplicate_keys} duplicate key(s) within the same section; the last occurrence won"
            ));
        }

        let (headers, raw_rows): (Vec<String>, Vec<Vec<String>>) = if has_explicit_section {
            report.info(format!("{} section(s) detected; one row per section", buckets.len()));
            let mut key_set: BTreeSet<String> = BTreeSet::new();
            for b in &buckets {
                for (k, _) in &b.pairs {
                    key_set.insert(k.clone());
                }
            }
            let mut headers = vec!["section".to_string()];
            headers.extend(key_set);
            let rows = buckets
                .iter()
                .map(|b| {
                    let mut row = vec![b.name.clone()];
                    for h in &headers[1..] {
                        row.push(b.pairs.iter().find(|(k, _)| k == h).map(|(_, v)| v.clone()).unwrap_or_default());
                    }
                    row
                })
                .collect();
            (headers, rows)
        } else {
            let bucket = &buckets[0];
            let headers: Vec<String> = bucket.pairs.iter().map(|(k, _)| k.clone()).collect();
            let row: Vec<String> = bucket.pairs.iter().map(|(_, v)| v.clone()).collect();
            (headers, vec![row])
        };

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
    fn a_log_timestamp_is_not_mistaken_for_a_key() {
        assert!(!is_kv_line("09:15:02 INFO started"));
        assert!(!looks_like_key("09:15:02 INFO started"));
    }

    #[test]
    fn a_url_query_string_is_not_mistaken_for_a_key() {
        assert!(!is_kv_line("GET /search?q=rust&page=2 HTTP/1.1"));
    }

    #[test]
    fn a_plain_key_value_line_is_recognized() {
        assert!(is_kv_line("DATABASE_URL=postgres://localhost/app"));
        assert!(is_kv_line("export API_KEY=abc123"));
        assert!(is_kv_line("timeout = 30"));
    }

    #[test]
    fn section_headers_are_recognized() {
        assert!(is_section_line("[default]"));
        assert!(!is_section_line("[incomplete"));
        assert!(!is_section_line("not a section"));
    }
}
