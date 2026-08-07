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
