//! Idempotence: cleaning an already-clean file must be a no-op. This
//! matters a lot for a pipeline tool — if re-running tidyloom on its own
//! output kept changing things (re-ordering columns, re-typing values,
//! adding/dropping rows), it would be unsafe to run more than once on the
//! same data, which is exactly the kind of surprise a CI/CD step can't
//! tolerate.
//!
//! Each test cleans a chaotic fixture to CSV, then cleans that CSV output
//! again, and asserts the two outputs are byte-for-byte identical.

use assert_cmd::Command;
use std::path::{Path, PathBuf};

fn fixture(dir: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures").join(dir).join(name)
}

fn clean_to(input: &Path, output: &Path) {
    Command::cargo_bin("tidyloom").unwrap().arg("clean").arg(input).arg("--output").arg(output).assert().success();
}

fn assert_second_pass_is_a_no_op(input: &Path) {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("first.csv");
    let second = tmp.path().join("second.csv");

    clean_to(input, &first);
    clean_to(&first, &second);

    let first_text = std::fs::read_to_string(&first).unwrap();
    let second_text = std::fs::read_to_string(&second).unwrap();
    assert_eq!(first_text, second_text, "cleaning {} a second time changed the output", first.display());
}

#[test]
fn csv_cleaning_is_idempotent() {
    assert_second_pass_is_a_no_op(&fixture("csv", "semicolon_ragged.csv"));
    assert_second_pass_is_a_no_op(&fixture("csv", "comma_quoted_extra.csv"));
    assert_second_pass_is_a_no_op(&fixture("csv", "tab_missing_cols.csv"));
}

#[test]
fn xlsx_cleaned_to_csv_is_idempotent_on_the_second_pass() {
    assert_second_pass_is_a_no_op(&fixture("xlsx", "junk_rows_and_merged_cells.xlsx"));
    assert_second_pass_is_a_no_op(&fixture("xlsx", "leading_blank_and_title_rows.xlsx"));
}

#[test]
fn json_cleaned_to_csv_is_idempotent_on_the_second_pass() {
    assert_second_pass_is_a_no_op(&fixture("json", "wrapped_items.json"));
    assert_second_pass_is_a_no_op(&fixture("json", "single_object.json"));
}

#[test]
fn fixed_width_cleaning_is_idempotent() {
    assert_second_pass_is_a_no_op(&fixture("fixed", "numeric_report.txt"));
}
