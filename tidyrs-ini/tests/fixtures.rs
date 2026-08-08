use tidyrs_core::{ParseOptions, TidyParser, TidyValue};
use tidyrs_ini::IniParser;

fn fixture(dir: &str, name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures").join(dir).join(name);
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {dir}/{name}: {e}"))
}

fn col<'a>(headers: &[String], row: &'a [TidyValue], name: &str) -> &'a TidyValue {
    let idx = headers
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("no column '{name}' in {headers:?}"));
    &row[idx]
}

#[test]
fn a_trailing_comment_after_a_quoted_value_is_stripped() {
    // Regression (found via external QA testing): a trailing `; comment`
    // used to become part of the value instead of being stripped —
    // `name = "Mon Application"  ; commentaire` came out as
    // `"Mon Application"  ; commentaire` (quotes and all), not
    // `Mon Application`.
    let bytes = b"name = \"Mon Application\"  ; commentaire en fin de ligne\nversion = 1.0\n".to_vec();
    let parser = IniParser::new();
    let outcome = parser.parse(&bytes, "app.ini", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(
        col(&table.headers, &table.rows[0], "name"),
        &TidyValue::Text("Mon Application".to_string())
    );
}

#[test]
fn a_comment_marker_without_preceding_whitespace_stays_part_of_the_value() {
    // The trailing-comment strip must not fire on a marker that's clearly
    // part of the value itself (a URL fragment, a password) — only a
    // marker preceded by whitespace is treated as a real comment.
    let bytes = b"page_url = http://example.com/path#section\n".to_vec();
    let parser = IniParser::new();
    let outcome = parser.parse(&bytes, "app.ini", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(
        col(&table.headers, &table.rows[0], "page_url"),
        &TidyValue::Text("http://example.com/path#section".to_string())
    );
}

#[test]
fn a_comment_marker_inside_quotes_stays_part_of_the_value() {
    let bytes = b"note = \"call me at 555-1234 ; ask for Bob\"\n".to_vec();
    let parser = IniParser::new();
    let outcome = parser.parse(&bytes, "app.ini", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(
        col(&table.headers, &table.rows[0], "note"),
        &TidyValue::Text("call me at 555-1234 ; ask for Bob".to_string())
    );
}

#[test]
fn sectioned_ini_produces_one_row_per_section() {
    let bytes = fixture("ini", "database.ini");
    let parser = IniParser::new();
    let outcome = parser.parse(&bytes, "database.ini", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 3);
    assert!(table.headers.contains(&"section".to_string()));

    let default_row = table
        .rows
        .iter()
        .find(|r| col(&table.headers, r, "section") == &TidyValue::Text("default".to_string()))
        .unwrap();
    assert_eq!(col(&table.headers, default_row, "port"), &TidyValue::Int(5432));
    assert_eq!(col(&table.headers, default_row, "timeout"), &TidyValue::Int(30));

    let staging_row = table
        .rows
        .iter()
        .find(|r| col(&table.headers, r, "section") == &TidyValue::Text("staging".to_string()))
        .unwrap();
    // "staging" has no "ssl" key at all -> filled with Null, unlike "production" which has ssl=true.
    assert_eq!(col(&table.headers, staging_row, "ssl"), &TidyValue::Null);

    let production_row = table
        .rows
        .iter()
        .find(|r| col(&table.headers, r, "section") == &TidyValue::Text("production".to_string()))
        .unwrap();
    assert_eq!(col(&table.headers, production_row, "ssl"), &TidyValue::Bool(true));
}

#[test]
fn flat_ini_with_no_sections_becomes_one_row() {
    let bytes = fixture("ini", "simple.ini");
    let parser = IniParser::new();
    let outcome = parser.parse(&bytes, "simple.ini", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 1);
    assert!(
        !table.headers.contains(&"section".to_string()),
        "a flat file shouldn't get a synthetic section column"
    );
    assert_eq!(col(&table.headers, &table.rows[0], "app_name"), &TidyValue::Text("tidyloom".to_string()));
    assert_eq!(col(&table.headers, &table.rows[0], "max_workers"), &TidyValue::Int(4));
    assert_eq!(col(&table.headers, &table.rows[0], "debug"), &TidyValue::Bool(false));
}

#[test]
fn dot_env_file_with_export_prefix_and_quoted_values_is_parsed() {
    let bytes = fixture("env", "app.env");
    let parser = IniParser::new();
    let outcome = parser.parse(&bytes, "app.env", &ParseOptions::new()).unwrap();
    let table = &outcome.tables[0];

    assert_eq!(table.rows.len(), 1);
    assert_eq!(
        col(&table.headers, &table.rows[0], "DATABASE_URL"),
        &TidyValue::Text("postgres://localhost:5432/app".to_string())
    );
    // Quotes around the value must be stripped.
    assert_eq!(
        col(&table.headers, &table.rows[0], "API_KEY"),
        &TidyValue::Text("abc123secret".to_string())
    );
    assert_eq!(col(&table.headers, &table.rows[0], "MAX_CONNECTIONS"), &TidyValue::Int(10));
    assert_eq!(outcome.report.detected_format, "env");
}

#[test]
fn sniff_recognizes_ini_extension() {
    let bytes = fixture("ini", "database.ini");
    let parser = IniParser::new();
    assert!(parser.sniff(&bytes, Some("database.ini")) > 0.6);
}

#[test]
fn sniff_recognizes_genuine_ini_content_with_no_filename_hint() {
    let bytes = fixture("ini", "database.ini");
    let parser = IniParser::new();
    assert!(parser.sniff(&bytes, None) > 0.5);
}
