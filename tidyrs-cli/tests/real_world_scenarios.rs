//! Real-world scenario tests: end-to-end workflows against larger,
//! deliberately messy fixtures (fixtures/real_world/, regenerated via
//! `cargo run -p tidyrs-cli --example gen_real_world_fixtures`) that mix
//! several kinds of mess in the same file the way an actual export would,
//! rather than isolating one behavior per fixture like the rest of the
//! test suite does. These exist to catch problems that only show up at
//! realistic size/variety — one of these (a single-column Excel sheet
//! losing all its data rows) found a real bug during development; see
//! tidyrs-xlsx's `single_column_sheet_keeps_all_its_data_rows` test.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/real_world").join(name)
}

fn tidyloom() -> Command {
    Command::cargo_bin("tidyloom").unwrap()
}

// ---------------------------------------------------------------------
// Scenario: messy sales CSV export, cleaned and checked for basic sanity
// ---------------------------------------------------------------------

#[test]
fn messy_sales_csv_cleans_without_dropping_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("sales_clean.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("sales_export_messy.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--verbose-report")
        .assert()
        .success()
        .stdout(predicate::str::contains("detected: csv"))
        .stdout(predicate::str::contains("inconsistent column count")); // the generator injects ragged rows

    let content = std::fs::read_to_string(&out).unwrap();
    let mut lines = content.lines();
    let header = lines.next().unwrap();
    assert_eq!(header, "order_id,customer_name,order_date,amount,currency,status,notes");

    // 130 generated rows, blank lines dropped, no row lost or duplicated
    // beyond that: every remaining line must have exactly 7 comma-
    // separated fields (ragged rows get padded/truncated, not dropped).
    let data_lines: Vec<&str> = lines.collect();
    assert_eq!(data_lines.len(), 130);
    for line in &data_lines {
        let mut rdr = csv::ReaderBuilder::new().has_headers(false).from_reader(line.as_bytes());
        let record = rdr.records().next().unwrap().unwrap();
        assert_eq!(record.len(), 7, "malformed row: {line:?}");
    }
}

#[test]
fn messy_sales_csv_second_pass_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("pass1.csv");
    let second = tmp.path().join("pass2.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("sales_export_messy.csv"))
        .arg("--output")
        .arg(&first)
        .assert()
        .success();
    tidyloom().arg("clean").arg(&first).arg("--output").arg(&second).assert().success();

    assert_eq!(std::fs::read_to_string(&first).unwrap(), std::fs::read_to_string(&second).unwrap());
}

#[test]
fn messy_sales_csv_streaming_preserves_original_number_formatting_unlike_in_memory_mode() {
    // Real finding from this scenario: on genuinely varied data (unlike
    // the uniformly-formatted synthetic fixture the existing
    // `stream_flag_produces_identical_output_to_the_in_memory_path` CLI
    // test uses), --stream does NOT byte-for-byte match the in-memory
    // path. This is real, documented behavior working as intended, not a
    // bug: --stream skips per-cell type inference (see
    // tidyrs-csv/src/stream.rs's module docs) and writes each field's
    // original text straight through, while the in-memory path parses
    // numbers into `TidyValue::Float`/`Int` and re-serializes them —
    // which silently normalizes formatting quirks like "922" (no
    // decimals) into "922" (Int, unchanged) but "3756.90" into "3756.9"
    // (Float, trailing zero dropped) and "3848.70" into "3848.7". Both
    // outputs are "correct" in the sense that neither loses or corrupts
    // data, but they are not interchangeable representations, so a
    // pipeline that switches between the two modes should not assume
    // identical output for a column with mixed decimal-place formatting.
    let tmp = tempfile::tempdir().unwrap();
    let normal = tmp.path().join("normal.csv");
    let streamed = tmp.path().join("streamed.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("sales_export_messy.csv"))
        .arg("--output")
        .arg(&normal)
        .assert()
        .success();
    tidyloom()
        .arg("clean")
        .arg(fixture("sales_export_messy.csv"))
        .arg("--output")
        .arg(&streamed)
        .arg("--stream")
        .assert()
        .success();

    let normal_content = std::fs::read_to_string(&normal).unwrap();
    let streamed_content = std::fs::read_to_string(&streamed).unwrap();

    // Same row/column shape either way...
    assert_eq!(normal_content.lines().count(), streamed_content.lines().count());
    assert_eq!(normal_content.lines().next(), streamed_content.lines().next());

    // ...but streamed output must contain the untouched original text
    // "3756.90" (not re-normalized to "3756.9"), proving --stream really
    // does skip type-driven reformatting on real messy data.
    assert!(
        streamed_content.contains("3756.90"),
        "streaming should preserve the original \"3756.90\" formatting verbatim"
    );
    assert!(
        normal_content.contains("3756.9,USD") && !normal_content.contains("3756.90,USD"),
        "in-memory mode should have re-normalized \"3756.90\" to \"3756.9\" via float parsing"
    );
}

// ---------------------------------------------------------------------
// Scenario: schema validation as a CI/CD data-quality gate
// ---------------------------------------------------------------------

fn strict_amount_schema(dir: &Path) -> PathBuf {
    let path = dir.join("schema.json");
    std::fs::write(
        &path,
        r#"{
            "columns": [
                {"name": "order_id", "type": "integer", "nullable": false},
                {"name": "amount", "type": "float", "nullable": false},
                {"name": "currency", "type": "text", "nullable": false}
            ],
            "strict": false
        }"#,
    )
    .unwrap();
    path
}

#[test]
fn schema_gate_warns_about_real_messy_amount_values_but_still_writes_output() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = strict_amount_schema(tmp.path());
    let out = tmp.path().join("clean.csv");

    // The generator deliberately renders some amounts as "$1234.56" or
    // "1234 56" — those fail a strict `float` check and should show up
    // as real, itemized schema violations, not be silently coerced.
    tidyloom()
        .arg("clean")
        .arg(fixture("sales_export_messy.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--schema")
        .arg(&schema)
        .assert()
        .success()
        .stderr(predicate::str::contains("violation"))
        .stderr(predicate::str::contains("amount"));

    assert!(out.exists(), "warn mode must still write output despite violations");
}

#[test]
fn schema_gate_rejects_and_writes_nothing_when_configured_to_reject() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = strict_amount_schema(tmp.path());
    let out = tmp.path().join("clean.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("sales_export_messy.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--schema")
        .arg(&schema)
        .arg("--on-schema-violation")
        .arg("reject")
        .assert()
        .failure();

    assert!(!out.exists(), "reject mode must not write output when there are violations");
}

// ---------------------------------------------------------------------
// Scenario: multi-sheet financial report workbook
// ---------------------------------------------------------------------

#[test]
fn financial_report_produces_one_csv_per_sheet_with_correct_row_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("report.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("q4_financial_report.xlsx"))
        .arg("--output")
        .arg(&out)
        .arg("--verbose-report")
        .assert()
        .success()
        .stdout(predicate::str::contains("found 3 sheet(s)"));

    let summary = std::fs::read_to_string(tmp.path().join("report_Summary.csv")).unwrap();
    // 5 regions + 1 TOTAL row + header
    assert_eq!(summary.lines().count(), 7);
    assert!(summary.contains("TOTAL"));

    let detail = std::fs::read_to_string(tmp.path().join("report_Regional_Detail.csv")).unwrap();
    // 3 regions x 3 cities + header, and the merged "region" column must
    // have been forward-filled onto every row of its group, not just the
    // first (exact merge-region filling, exercised at realistic size).
    assert_eq!(detail.lines().count(), 10);
    for line in detail.lines().skip(1) {
        assert!(!line.starts_with(','), "region column should never be blank after merge-fill: {line:?}");
    }

    // The regression this whole fixture exists for: a single-column
    // "Notes" sheet must keep all its data rows, not just the header.
    let notes = std::fs::read_to_string(tmp.path().join("report_Notes.csv")).unwrap();
    assert_eq!(notes.lines().count(), 5); // header + 4 notes
}

// ---------------------------------------------------------------------
// Scenario: nested JSON API export, with and without exploding arrays
// ---------------------------------------------------------------------

#[test]
fn nested_orders_json_default_mode_keeps_one_row_per_order() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("orders.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("orders_nested.json"))
        .arg("--output")
        .arg(&out)
        .assert()
        .success();

    let content = std::fs::read_to_string(&out).unwrap();
    // 40 generated orders -> 40 rows, regardless of how many line items
    // or which optional fields each one happened to have.
    assert_eq!(content.lines().count(), 41);
}

#[test]
fn nested_orders_json_explode_mode_expands_line_items_into_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("orders_exploded.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("orders_nested.json"))
        .arg("--output")
        .arg(&out)
        .arg("--array-mode")
        .arg("explode")
        .assert()
        .success();

    let content = std::fs::read_to_string(&out).unwrap();
    let row_count = content.lines().count() - 1; // minus header
                                                 // Every order has 1-3 items, so exploding must produce at least as
                                                 // many rows as orders (40), and typically more.
    assert!(row_count >= 40, "expected exploded row count >= order count, got {row_count}");
    assert!(content.contains("items.sku"));
}

// ---------------------------------------------------------------------
// Scenario: whitespace-separated server log
// ---------------------------------------------------------------------

#[test]
fn server_log_whitespace_mode_extracts_structured_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("log.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("server_activity.log"))
        .arg("--output")
        .arg(&out)
        .arg("--format")
        .arg("fixed")
        .arg("--mode")
        .arg("whitespace")
        .assert()
        .success();

    let content = std::fs::read_to_string(&out).unwrap();
    assert_eq!(content.lines().count(), 81); // header + 80 log lines
    let first_data_line = content.lines().nth(1).unwrap();
    // First two whitespace-separated tokens on every generated line are
    // always a date and a time.
    assert!(first_data_line.starts_with("2026-01-"));
}

// ---------------------------------------------------------------------
// Scenario: a folder of mixed formats, batch-processed overnight, with
// one corrupted file that must not take the whole run down.
// ---------------------------------------------------------------------

#[test]
fn mixed_batch_folder_survives_one_corrupted_file() {
    let tmp = tempfile::tempdir().unwrap();
    let input_dir = tmp.path().join("incoming");
    std::fs::create_dir_all(&input_dir).unwrap();

    for name in [
        "sales_export_messy.csv",
        "orders_nested.json",
        "server_activity.log",
        "q4_financial_report.xlsx",
    ] {
        std::fs::copy(fixture(name), input_dir.join(name)).unwrap();
    }
    // A corrupted/truncated file that landed in the drop folder overnight
    // — the kind of thing a real unattended pipeline has to survive.
    std::fs::write(
        input_dir.join("corrupted.csv.xlsx"),
        b"not actually a valid xlsx file, just garbage bytes",
    )
    .unwrap();

    let out_dir = tmp.path().join("clean");
    let report_dir = tmp.path().join("reports");

    tidyloom()
        .arg("clean")
        .arg("--batch")
        .arg(&input_dir)
        .arg("--output-dir")
        .arg(&out_dir)
        .arg("--report-dir")
        .arg(&report_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("batch complete: 4 file(s) cleaned, 1 failure(s)"));

    // The three well-formed single-table files produced output...
    assert!(out_dir.join("sales_export_messy.csv").exists());
    assert!(out_dir.join("orders_nested.csv").exists());
    assert!(out_dir.join("server_activity.csv").exists());
    // ...the multi-sheet workbook produced its per-sheet outputs too...
    assert!(out_dir.join("q4_financial_report_Summary.csv").exists());
    // ...and the corrupted file did NOT produce output, but also didn't
    // stop the rest of the batch from completing.
    assert!(!out_dir.join("corrupted.csv.csv").exists());
}

// ---------------------------------------------------------------------
// Scenario: dry-run before committing to a change, then applying it
// ---------------------------------------------------------------------

#[test]
fn dry_run_preview_then_apply_workflow_on_real_data() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("orders.csv");

    // 1. Dry-run: nothing written yet.
    tidyloom()
        .arg("clean")
        .arg(fixture("orders_nested.json"))
        .arg("--output")
        .arg(&out)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("would create new file"));
    assert!(!out.exists());

    // 2. Apply for real.
    tidyloom()
        .arg("clean")
        .arg(fixture("orders_nested.json"))
        .arg("--output")
        .arg(&out)
        .assert()
        .success();
    assert!(out.exists());

    // 3. Dry-run again against the now-existing output: unchanged.
    tidyloom()
        .arg("clean")
        .arg(fixture("orders_nested.json"))
        .arg("--output")
        .arg(&out)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("unchanged"));
}

// ---------------------------------------------------------------------
// Scenario: project-wide tidyloom.toml defaults used in a real pipeline
// ---------------------------------------------------------------------

#[test]
fn project_config_defaults_apply_to_a_real_file_without_repeating_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("tidyloom.toml");
    std::fs::write(
        &config,
        r#"
[defaults]
array_mode = "explode"
verbose_report = true
"#,
    )
    .unwrap();
    let out = tmp.path().join("orders.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("orders_nested.json"))
        .arg("--output")
        .arg(&out)
        .arg("--config")
        .arg(&config)
        .assert()
        .success()
        // The fixture is a top-level JSON array (no wrapper key), so the
        // "used array found under key" note never fires — instead, prove
        // both config-supplied defaults applied together: verbose_report
        // made this note visible at all, and array_mode=explode is what
        // it's reporting on.
        .stdout(predicate::str::contains("array_mode=explode: 40 input record(s) expanded into 81 row(s)"));

    let content = std::fs::read_to_string(&out).unwrap();
    let row_count = content.lines().count() - 1;
    assert!(
        row_count >= 40,
        "config-supplied array_mode=explode should have applied, got {row_count} rows"
    );
}

// ---------------------------------------------------------------------
// Scenario: account export in YAML, with an inconsistently-shaped
// optional field across records (nested mapping / plain scalar / absent)
// ---------------------------------------------------------------------

#[test]
fn accounts_yaml_flattens_an_inconsistently_shaped_field_without_dropping_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("accounts.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("accounts_export.yaml"))
        .arg("--output")
        .arg(&out)
        .arg("--verbose-report")
        .assert()
        .success()
        .stdout(predicate::str::contains("detected: yaml"))
        .stdout(predicate::str::contains("missing fields were filled with null"));

    let content = std::fs::read_to_string(&out).unwrap();
    let mut lines = content.lines();
    let header = lines.next().unwrap();
    // "billing" is sometimes a {method, amount} mapping, sometimes the
    // plain scalar "invoiced", sometimes absent entirely — the generator
    // exercises all three in the same file, the same real-world drift
    // gen_orders_json's "shipping" field already covers for JSON.
    for col in ["id", "name", "plan", "active", "billing", "billing.method", "billing.amount"] {
        assert!(
            header.split(',').any(|h| h == col),
            "missing expected column '{col}' in header {header:?}"
        );
    }

    // 45 generated accounts -> 45 rows, regardless of which optional
    // shape "billing" happened to take for any given record.
    assert_eq!(lines.count(), 45);
}

#[test]
fn accounts_yaml_second_pass_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("pass1.csv");
    let second = tmp.path().join("pass2.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("accounts_export.yaml"))
        .arg("--output")
        .arg(&first)
        .assert()
        .success();
    tidyloom().arg("clean").arg(&first).arg("--output").arg(&second).assert().success();

    assert_eq!(std::fs::read_to_string(&first).unwrap(), std::fs::read_to_string(&second).unwrap());
}

// ---------------------------------------------------------------------
// Scenario: multi-environment service config (.ini) and a deployment
// secrets file (.env) from the same real-world pipeline
// ---------------------------------------------------------------------

#[test]
fn services_ini_produces_one_row_per_environment_with_gaps_where_keys_are_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("services.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("services.ini"))
        .arg("--output")
        .arg(&out)
        .arg("--verbose-report")
        .assert()
        .success()
        .stdout(predicate::str::contains("detected: ini"))
        .stdout(predicate::str::contains("4 section(s) detected"));

    let content = std::fs::read_to_string(&out).unwrap();
    let mut rows: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    let mut lines = content.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let section_idx = header.iter().position(|h| *h == "section").unwrap();
    for line in lines {
        let fields: Vec<&str> = line.split(',').collect();
        rows.insert(fields[section_idx], fields);
    }
    assert_eq!(rows.len(), 4);

    let timeout_idx = header.iter().position(|h| *h == "timeout").unwrap();
    let ssl_idx = header.iter().position(|h| *h == "ssl").unwrap();

    // "qa" deliberately has no timeout key in the source file — that gap
    // must come through as a real empty field, not silently borrow
    // another section's value or disappear from the row entirely.
    assert_eq!(rows["qa"][timeout_idx], "");
    assert_eq!(rows["qa"][ssl_idx], "");
    // "production" has both timeout and ssl set.
    assert_ne!(rows["production"][timeout_idx], "");
    assert_eq!(rows["production"][ssl_idx], "true");
    // "dev" has no ssl key anywhere in its section.
    assert_eq!(rows["dev"][ssl_idx], "");
}

#[test]
fn deploy_env_is_parsed_as_a_single_flat_record_with_export_prefix_and_quotes_handled() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("deploy.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("deploy.env"))
        .arg("--output")
        .arg(&out)
        .arg("--verbose-report")
        .assert()
        .success()
        .stdout(predicate::str::contains("detected: env"));

    let content = std::fs::read_to_string(&out).unwrap();
    let mut lines = content.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let row: Vec<&str> = lines.next().unwrap().split(',').collect();
    assert!(lines.next().is_none(), "a flat .env file should produce exactly one row");

    // Both the plain and the "export "-prefixed vars must appear as
    // ordinary columns — the prefix is a shell-sourcing convention, not
    // part of the key.
    for key in [
        "DATABASE_URL",
        "REDIS_URL",
        "API_KEY",
        "MAX_WORKERS",
        "FEATURE_NEW_CHECKOUT",
        "SUPPORT_EMAIL",
    ] {
        assert!(header.contains(&key), "missing expected column '{key}' in header {header:?}");
    }
    let email_idx = header.iter().position(|h| *h == "SUPPORT_EMAIL").unwrap();
    // Source has SUPPORT_EMAIL='support@example.com' (single-quoted) —
    // the quotes must be stripped, not carried into the value.
    assert_eq!(row[email_idx], "support@example.com");
    let url_idx = header.iter().position(|h| *h == "DATABASE_URL").unwrap();
    assert!(row[url_idx].starts_with("postgres://"));
}

// ---------------------------------------------------------------------
// Scenario: a small shop's SQLite database (customers/products/orders),
// the kind of thing exported from an internal admin tool for analysis
// ---------------------------------------------------------------------

#[test]
fn shop_database_produces_one_csv_per_table_with_correct_row_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("shop.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("shop.db"))
        .arg("--output")
        .arg(&out)
        .arg("--verbose-report")
        .assert()
        .success()
        .stdout(predicate::str::contains("detected: sqlite"))
        .stdout(predicate::str::contains("found 3 table(s): customers, orders, products"));

    let customers = std::fs::read_to_string(tmp.path().join("shop_customers.csv")).unwrap();
    assert_eq!(customers.lines().count(), 31); // header + 30 customers

    let products = std::fs::read_to_string(tmp.path().join("shop_products.csv")).unwrap();
    assert_eq!(products.lines().count(), 16); // header + 15 products

    let orders = std::fs::read_to_string(tmp.path().join("shop_orders.csv")).unwrap();
    assert_eq!(orders.lines().count(), 61); // header + 60 orders

    // Not every customer has a verified email on file — that gap must
    // survive as a real empty field somewhere in the table, not get
    // silently dropped or crash the whole read.
    assert!(
        customers.lines().skip(1).any(|line| line.split(',').nth(2) == Some("")),
        "expected at least one customer row with a missing email"
    );
}

#[test]
fn shop_database_table_option_extracts_only_the_requested_table() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("shop.csv");

    tidyloom()
        .arg("clean")
        .arg(fixture("shop.db"))
        .arg("--output")
        .arg(&out)
        .arg("--table")
        .arg("orders")
        .assert()
        .success();

    // --table restricts to one table, so this is single-file mode: the
    // plain --output path is used directly, no _orders suffix.
    assert!(out.exists());
    assert!(!tmp.path().join("shop_customers.csv").exists());
    assert!(!tmp.path().join("shop_products.csv").exists());
    let content = std::fs::read_to_string(&out).unwrap();
    assert_eq!(content.lines().count(), 61); // header + 60 orders
}

// ---------------------------------------------------------------------
// Scenario: an overnight batch drop folder containing every new format
// added alongside the original CSV/Excel/JSON/log set
// ---------------------------------------------------------------------

#[test]
fn mixed_batch_folder_handles_yaml_ini_env_and_sqlite_together() {
    let tmp = tempfile::tempdir().unwrap();
    let input_dir = tmp.path().join("incoming");
    std::fs::create_dir_all(&input_dir).unwrap();

    for name in ["accounts_export.yaml", "services.ini", "deploy.env", "shop.db"] {
        std::fs::copy(fixture(name), input_dir.join(name)).unwrap();
    }

    let out_dir = tmp.path().join("clean");
    tidyloom()
        .arg("clean")
        .arg("--batch")
        .arg(&input_dir)
        .arg("--output-dir")
        .arg(&out_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("batch complete: 4 file(s) cleaned, 0 failure(s)"));

    assert!(out_dir.join("accounts_export.csv").exists());
    assert!(out_dir.join("services.csv").exists());
    assert!(out_dir.join("deploy.csv").exists());
    // shop.db has 3 tables -> per-table batch output, same naming scheme
    // the multi-sheet Excel case already uses.
    assert!(out_dir.join("shop_customers.csv").exists());
    assert!(out_dir.join("shop_orders.csv").exists());
    assert!(out_dir.join("shop_products.csv").exists());
}
