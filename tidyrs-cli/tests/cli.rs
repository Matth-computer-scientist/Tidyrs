use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;

fn fixture(dir: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures").join(dir).join(name)
}

#[test]
fn cleans_a_single_csv_file() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("clean.csv");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .args(["clean"])
        .arg(fixture("csv", "semicolon_ragged.csv"))
        .arg("--output")
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("detected: csv"));

    let content = std::fs::read_to_string(&out).unwrap();
    assert!(content.starts_with("name,age,city,notes"));
    assert!(content.contains("Alice,30,Paris,likes tea"));
}

#[test]
fn stream_flag_produces_identical_output_to_the_in_memory_path() {
    let tmp = tempfile::tempdir().unwrap();
    let streamed_out = tmp.path().join("streamed.csv");
    let normal_out = tmp.path().join("normal.csv");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "semicolon_ragged.csv"))
        .arg("--output")
        .arg(&streamed_out)
        .arg("--stream")
        .arg("--verbose-report")
        .assert()
        .success()
        .stdout(predicate::str::contains("streaming mode"));

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "semicolon_ragged.csv"))
        .arg("--output")
        .arg(&normal_out)
        .assert()
        .success();

    let streamed = std::fs::read_to_string(&streamed_out).unwrap();
    let normal = std::fs::read_to_string(&normal_out).unwrap();
    assert_eq!(streamed, normal);
}

#[test]
fn stream_flag_falls_back_for_non_csv_output() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.json");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--stream")
        .assert()
        .success()
        .stderr(predicate::str::contains("falling back to the normal in-memory path"));

    assert!(out.exists());
}

#[test]
fn schema_violation_warns_but_still_writes_output_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let schema_path = tmp.path().join("schema.json");
    std::fs::write(&schema_path, r#"{"columns":[{"name":"age","type":"integer","nullable":false}]}"#).unwrap();
    let out = tmp.path().join("out.csv");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "semicolon_ragged.csv")) // Bob's age is empty
        .arg("--output")
        .arg(&out)
        .arg("--schema")
        .arg(&schema_path)
        .assert()
        .success()
        .stderr(predicate::str::contains("violation"));

    assert!(out.exists());
}

#[test]
fn schema_violation_with_reject_fails_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let schema_path = tmp.path().join("schema.json");
    std::fs::write(&schema_path, r#"{"columns":[{"name":"age","type":"integer","nullable":false}]}"#).unwrap();
    let out = tmp.path().join("out.csv");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "semicolon_ragged.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--schema")
        .arg(&schema_path)
        .arg("--on-schema-violation")
        .arg("reject")
        .assert()
        .failure();

    assert!(!out.exists());
}

#[test]
fn dry_run_previews_a_new_file_without_creating_it() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("would_exist.csv");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("would create new file"))
        .stdout(predicate::str::contains("id,product,price,in_stock"));

    assert!(!out.exists(), "dry-run must not write any file");
}

#[test]
fn dry_run_reports_unchanged_when_output_already_matches() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.csv");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .assert()
        .success();
    let before = std::fs::read_to_string(&out).unwrap();

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("unchanged"));

    let after = std::fs::read_to_string(&out).unwrap();
    assert_eq!(before, after, "dry-run must not modify an existing file either");
}

#[test]
fn dry_run_shows_a_diff_against_a_different_existing_output() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.csv");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .assert()
        .success();

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "semicolon_ragged.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("would change"))
        .stdout(predicate::str::contains("- id,product,price,in_stock"))
        .stdout(predicate::str::contains("+ name,age,city,notes"));

    // The old content is still there — dry-run never overwrites.
    let content = std::fs::read_to_string(&out).unwrap();
    assert!(content.starts_with("id,product,price,in_stock"));
}

#[test]
fn config_file_supplies_defaults_that_cli_flags_can_override() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("tidyloom.toml");
    std::fs::write(
        &config_path,
        r#"
[defaults]
delimiter = ","
verbose_report = true
"#,
    )
    .unwrap();
    let out = tmp.path().join("out.csv");

    // The fixture is pipe-delimited; the config forces comma, so with no
    // CLI override the whole line becomes one column.
    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--config")
        .arg(&config_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("detected delimiter: ','")); // only printed because verbose_report came from config

    let content = std::fs::read_to_string(&out).unwrap();
    assert!(
        content.lines().next().unwrap().contains('|'),
        "comma delimiter from config should have left the pipes intact in one column"
    );

    // Now pass --delimiter explicitly: the CLI flag must win over the config file.
    let out2 = tmp.path().join("out2.csv");
    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out2)
        .arg("--config")
        .arg(&config_path)
        .arg("--delimiter")
        .arg("|")
        .assert()
        .success();

    let content2 = std::fs::read_to_string(&out2).unwrap();
    assert_eq!(content2.lines().next().unwrap(), "id,product,price,in_stock");
}

#[test]
fn missing_config_file_is_not_an_error_when_not_explicitly_requested() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.csv");

    // No --config passed and no ./tidyloom.toml in the test's cwd:
    // should just behave like there's no config at all.
    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .assert()
        .success();

    assert!(out.exists());
}

#[test]
fn explicit_missing_config_path_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.csv");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--config")
        .arg(tmp.path().join("does_not_exist.toml"))
        .assert()
        .failure();
}

#[test]
fn log_format_json_emits_one_parseable_json_object_per_line_on_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.csv");

    let assert = Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("--log-format")
        .arg("json")
        .arg("clean")
        .arg(fixture("csv", "semicolon_ragged.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--verbose-report")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!lines.is_empty(), "expected at least one JSON log line on stdout");

    let mut found_summary = false;
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("line was not valid JSON: {e}\nline: {line}"));
        assert!(parsed.get("level").is_some(), "expected a 'level' field: {line}");
        assert!(
            parsed.get("fields").and_then(|f| f.get("message")).is_some(),
            "expected fields.message: {line}"
        );
        if parsed["fields"]["message"].as_str().unwrap_or("").contains("detected: csv") {
            found_summary = true;
            assert_eq!(parsed["fields"]["rows_out"], 4);
        }
    }
    assert!(found_summary, "expected the per-file summary line among the JSON log lines");
}

#[test]
fn writes_a_json_cleaning_report_next_to_the_output() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("clean.csv");
    let report = tmp.path().join("clean.report.json");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--report-file")
        .arg(&report)
        .assert()
        .success();

    let report_json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(report_json["detected_format"], "csv");
    assert_eq!(report_json["rows_out"], 3);
    assert!(!report_json["notes"].as_array().unwrap().is_empty());
}

#[test]
fn batch_mode_cleans_every_file_and_reports_are_per_file() {
    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("clean");
    let report_dir = tmp.path().join("reports");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg("--batch")
        .arg(fixture("csv", ""))
        .arg("--output-dir")
        .arg(&out_dir)
        .arg("--report-dir")
        .arg(&report_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("batch complete"))
        .stdout(predicate::str::contains("0 failure"));

    assert!(out_dir.join("semicolon_ragged.csv").exists());
    assert!(report_dir.join("semicolon_ragged.report.json").exists());
}

#[test]
fn batch_mode_disambiguates_output_files_that_share_a_stem() {
    // Regression: two input files with the same stem but different
    // formats (data.csv, data.json) used to both resolve to the same
    // output path (data.csv) — the second silently overwrote the first,
    // with the batch summary still reporting both as successfully
    // cleaned. Found via manual QA testing, not the automated suite.
    let tmp = tempfile::tempdir().unwrap();
    let in_dir = tmp.path().join("in");
    std::fs::create_dir_all(&in_dir).unwrap();
    std::fs::write(in_dir.join("data.csv"), "id,name\n1,FromCSV\n").unwrap();
    std::fs::write(in_dir.join("data.json"), r#"[{"id": 2, "name": "FromJSON"}]"#).unwrap();
    let out_dir = tmp.path().join("clean");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg("--batch")
        .arg(&in_dir)
        .arg("--output-dir")
        .arg(&out_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("batch complete: 2 file(s) cleaned, 0 failure(s)"));

    // Both files must survive as distinct outputs, not one overwriting
    // the other.
    let from_csv = std::fs::read_to_string(out_dir.join("data_csv.csv")).unwrap();
    assert!(from_csv.contains("FromCSV"));
    let from_json = std::fs::read_to_string(out_dir.join("data_json.csv")).unwrap();
    assert!(from_json.contains("FromJSON"));
    // The plain "data.csv" name must not exist at all — a leftover file
    // there would mean one of the two inputs is still silently winning.
    assert!(!out_dir.join("data.csv").exists());
}

#[test]
fn batch_mode_skips_subdirectories_with_an_explicit_warning() {
    // Regression: --batch only reads the top level of the given
    // directory (std::fs::read_dir doesn't recurse) — a file inside a
    // subdirectory used to be silently excluded with zero indication
    // anything was skipped. Found via manual QA testing.
    let tmp = tempfile::tempdir().unwrap();
    let in_dir = tmp.path().join("in");
    let sub_dir = in_dir.join("sub");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::write(in_dir.join("top.csv"), "id,name\n1,Alice\n").unwrap();
    std::fs::write(sub_dir.join("nested.csv"), "id,name\n2,Bob\n").unwrap();
    let out_dir = tmp.path().join("clean");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg("--batch")
        .arg(&in_dir)
        .arg("--output-dir")
        .arg(&out_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("batch complete: 1 file(s) cleaned, 0 failure(s)"))
        .stderr(predicate::str::contains("subdirectory skipped"));

    assert!(out_dir.join("top.csv").exists());
    assert!(!out_dir.join("nested.csv").exists());
}

#[test]
fn json_report_severity_is_lowercase() {
    // Regression: CleaningNote's severity used to serialize as the
    // PascalCase Rust variant name ("Info"/"Warning") while every other
    // field in the report is snake_case/lowercase — an inconsistency a
    // downstream JSON consumer could trip on. Found via manual QA
    // testing.
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.csv");
    let report = tmp.path().join("report.json");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "semicolon_ragged.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--report-file")
        .arg(&report)
        .assert()
        .success();

    let content = std::fs::read_to_string(&report).unwrap();
    assert!(
        !content.contains("\"Info\"") && !content.contains("\"Warning\""),
        "severity must not be PascalCase: {content}"
    );
}

#[test]
fn forcing_the_wrong_format_fails_with_a_clear_error() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.csv");

    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("json", "single_object.json"))
        .arg("--output")
        .arg(&out)
        .arg("--format")
        .arg("xlsx")
        .assert()
        .failure();
}

#[test]
fn delimiter_flag_overrides_auto_detection() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.csv");

    // pipe_delimited.csv actually uses '|'; forcing ',' should produce a
    // single (wrong) column per row, proving the flag was honored rather
    // than auto-detection silently overriding it.
    Command::cargo_bin("tidyloom")
        .unwrap()
        .arg("clean")
        .arg(fixture("csv", "pipe_delimited.csv"))
        .arg("--output")
        .arg(&out)
        .arg("--delimiter")
        .arg(",")
        .assert()
        .success();

    let content = std::fs::read_to_string(&out).unwrap();
    let header_line = content.lines().next().unwrap();
    assert!(!header_line.contains(','), "expected no comma-split header, got: {header_line}");
}
