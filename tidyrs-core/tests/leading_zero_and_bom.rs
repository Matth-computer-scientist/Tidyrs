use tidyrs_core::{has_meaningful_leading_zero, strip_utf8_bom, type_columns, RuleBasedResolver, TidyValue};

// Regression (found via external QA testing): TidyValue::infer_from_str and
// the column-wide typing path both used a raw str::parse for numeric
// inference, which silently strips a meaningful leading zero — "00501"
// becomes 501, "007" becomes 7. Real, silent data corruption on postal
// codes, padded IDs, and phone extensions, with no warning anywhere in
// the pipeline.

#[test]
fn leading_zero_strings_are_detected() {
    assert!(has_meaningful_leading_zero("00501"));
    assert!(has_meaningful_leading_zero("007"));
    assert!(has_meaningful_leading_zero("-007"));
    // Not flagged: no leading zero at all, or a genuine decimal (the
    // character after the leading zero is '.', not another digit).
    assert!(!has_meaningful_leading_zero("501"));
    assert!(!has_meaningful_leading_zero("0"));
    assert!(!has_meaningful_leading_zero("0.5"));
    assert!(!has_meaningful_leading_zero("-0.5"));
    assert!(!has_meaningful_leading_zero(""));
}

#[test]
fn infer_from_str_keeps_a_leading_zero_code_as_text() {
    assert_eq!(TidyValue::infer_from_str("00501"), TidyValue::Text("00501".to_string()));
    assert_eq!(TidyValue::infer_from_str("007"), TidyValue::Text("007".to_string()));
    // An ordinary integer/decimal must still type normally.
    assert_eq!(TidyValue::infer_from_str("501"), TidyValue::Int(501));
    assert_eq!(TidyValue::infer_from_str("0.5"), TidyValue::Float(0.5));
    assert_eq!(TidyValue::infer_from_str("0"), TidyValue::Int(0));
}

#[test]
fn a_column_of_postal_codes_stays_text_even_when_confidently_typed_as_integer() {
    // The column-wide path (type_columns) is a second, independent place
    // the same bug could reappear: the *resolver* must not even classify
    // a leading-zero-heavy column as Integer in the first place (every
    // value here parses fine as i64, which used to be the only thing the
    // resolver checked), and convert_column must not strip a leading zero
    // even for a value in a column that *did* get typed Integer/Float.
    let headers = vec!["postal_code".to_string()];
    let raw_rows = vec![
        vec!["00501".to_string()],
        vec!["02134".to_string()],
        vec!["06390".to_string()],
        vec!["00544".to_string()],
    ];
    let resolver = RuleBasedResolver;
    let typed = type_columns(&headers, &raw_rows, &resolver);

    for (i, expected) in ["00501", "02134", "06390", "00544"].iter().enumerate() {
        assert_eq!(
            typed.rows[i][0],
            TidyValue::Text(expected.to_string()),
            "leading zero must survive in row {i}: {:?}",
            typed.rows[i]
        );
    }
}

// Regression (found via external QA testing): a UTF-8 byte-order mark is
// valid UTF-8 (decodes to U+FEFF) so it survives str::from_utf8/
// from_utf8_lossy unchanged and isn't whitespace, so it isn't trimmed
// either — left in place, it glues itself onto the first character of
// whatever a parser reads next (a CSV header's first column name, a
// JSON document's leading '{').

#[test]
fn strip_utf8_bom_removes_a_leading_bom() {
    let with_bom = b"\xEF\xBB\xBFid,name\n1,Alice\n";
    let stripped = strip_utf8_bom(with_bom);
    assert_eq!(stripped, b"id,name\n1,Alice\n");
}

#[test]
fn strip_utf8_bom_is_a_no_op_without_a_bom() {
    let plain = b"id,name\n1,Alice\n";
    assert_eq!(strip_utf8_bom(plain), plain);
}
