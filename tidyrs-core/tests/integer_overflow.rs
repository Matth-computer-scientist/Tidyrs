use tidyrs_core::{looks_like_a_whole_number, type_columns, RuleBasedResolver, TidyValue};

// Regression (found via external QA testing, round 4): an integer literal
// too big for i64 used to silently fall back to str::parse::<f64>, which
// only has ~15-17 significant decimal digits of precision — rounding the
// whole value to the nearest representable float instead of losing just
// the tail end. "9999999999999999999" (20 digits) becoming
// "10000000000000000000" with no warning anywhere in the pipeline is the
// same class of silent corruption as the leading-zero bug, just triggered
// by magnitude instead of a leading zero.

#[test]
fn looks_like_a_whole_number_distinguishes_integers_from_real_floats() {
    assert!(looks_like_a_whole_number("9999999999999999999"));
    assert!(looks_like_a_whole_number("-9223372036854775808"));
    assert!(looks_like_a_whole_number("501"));
    // Not flagged: a genuine decimal or scientific-notation literal is a
    // real float, and must still take the f64 path.
    assert!(!looks_like_a_whole_number("3.14"));
    assert!(!looks_like_a_whole_number("1e300"));
    assert!(!looks_like_a_whole_number(""));
    assert!(!looks_like_a_whole_number("-"));
}

#[test]
fn infer_from_str_keeps_an_i64_overflowing_integer_as_exact_text() {
    assert_eq!(
        TidyValue::infer_from_str("9999999999999999999"),
        TidyValue::Text("9999999999999999999".to_string())
    );
    // An ordinary integer, including the i64::MIN boundary value, must
    // still type normally as a native Int.
    assert_eq!(TidyValue::infer_from_str("501"), TidyValue::Int(501));
    assert_eq!(TidyValue::infer_from_str("-9223372036854775808"), TidyValue::Int(i64::MIN));
    // A genuine float that's merely large (not a whole-number overflow)
    // must still use the f64 path.
    assert_eq!(TidyValue::infer_from_str("1e300"), TidyValue::Float(1e300));
}

#[test]
fn a_column_mixing_an_overflowing_integer_with_real_floats_does_not_round_it() {
    // The column-wide path (type_columns) is a second, independent place
    // the same bug could reappear: if a column's majority-vote type lands
    // on Float (because most values are genuine decimals), the minority
    // whole-number-but-overflowing value must still be preserved exactly
    // rather than silently rounded through the column's own f64 parse.
    let headers = vec!["amount".to_string()];
    let raw_rows = vec![
        vec!["10.50".to_string()],
        vec!["20.25".to_string()],
        vec!["30.75".to_string()],
        vec!["9999999999999999999".to_string()],
    ];
    let resolver = RuleBasedResolver;
    let typed = type_columns(&headers, &raw_rows, &resolver);

    assert_eq!(typed.rows[0][0], TidyValue::Float(10.50));
    assert_eq!(
        typed.rows[3][0],
        TidyValue::Text("9999999999999999999".to_string()),
        "an i64-overflowing whole number in an otherwise-float column must keep its exact digits, got {:?}",
        typed.rows[3][0]
    );
}
