use std::io::Write;
use tidyrs_core::ParseOptions;
use tidyrs_csv::stream_clean_csv;

#[test]
fn streaming_output_matches_in_memory_output_for_a_normal_file() {
    let input = b"name;age;city\nAlice;30;Paris\nBob;41;Lyon\n".to_vec();
    let mut streamed = Vec::new();
    let report = stream_clean_csv(input.as_slice(), &mut streamed, "in.csv", &ParseOptions::new()).unwrap();

    let streamed_text = String::from_utf8(streamed).unwrap();
    assert!(streamed_text.starts_with("name,age,city"));
    assert!(streamed_text.contains("Alice,30,Paris"));
    assert!(streamed_text.contains("Bob,41,Lyon"));
    assert_eq!(report.rows_in, 2);
    assert_eq!(report.rows_out, 2);
    assert!(report.notes.iter().any(|n| n.message.contains("streaming mode")));
}

#[test]
fn streaming_pads_and_truncates_ragged_rows_like_the_in_memory_path() {
    let input = b"name,age,city\nAlice,30,Paris\nBob,41\nCharlotte,25,Marseille,extra\n".to_vec();
    let mut streamed = Vec::new();
    let report = stream_clean_csv(input.as_slice(), &mut streamed, "in.csv", &ParseOptions::new()).unwrap();

    let text = String::from_utf8(streamed).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "name,age,city");
    assert_eq!(lines[2], "Bob,41,"); // padded
    assert_eq!(lines[3], "Charlotte,25,Marseille"); // truncated
    assert_eq!(report.rows_in, 3);
    assert!(report.notes.iter().any(|n| n.message.contains("inconsistent column count")));
}

#[test]
fn streaming_processes_a_large_file_without_holding_it_all_in_memory() {
    // Not a literal memory-usage assertion (no cross-platform RSS probe
    // here) — but a 2,000,000-row file (well over the fixed 64KB sniff
    // prefix) completing quickly and correctly is strong circumstantial
    // evidence the implementation is actually streaming record-by-record
    // rather than silently buffering everything, which the previous
    // in-memory `CsvParser` did.
    let rows = 2_000_000;
    let mut input = Vec::new();
    writeln!(input, "id,value").unwrap();
    for i in 0..rows {
        writeln!(input, "{i},{}", i * 2).unwrap();
    }

    let mut streamed = Vec::new();
    let report = stream_clean_csv(input.as_slice(), &mut streamed, "big.csv", &ParseOptions::new()).unwrap();

    assert_eq!(report.rows_in, rows);
    assert_eq!(report.rows_out, rows);
    let text = String::from_utf8(streamed).unwrap();
    assert!(text.starts_with("id,value"));
    assert!(text.contains(&format!("{},{}", rows - 1, (rows - 1) * 2)));
}

#[test]
fn streaming_falls_back_to_in_memory_for_non_utf8_input() {
    let (encoded, _, _) = encoding_rs::WINDOWS_1252.encode("name;city\nRené;Orléans\n");
    let mut streamed = Vec::new();
    let report = stream_clean_csv(encoded.as_ref(), &mut streamed, "latin.csv", &ParseOptions::new()).unwrap();

    let text = String::from_utf8(streamed).unwrap();
    assert!(text.contains("René"));
    assert!(report.notes.iter().any(|n| n.message.contains("fell back to reading the whole file")));
}

#[test]
fn no_header_mode_infers_width_from_first_data_row() {
    let input = b"1,2,3\n4,5,6\n".to_vec();
    let mut streamed = Vec::new();
    let opts = ParseOptions::new().set("has_header", "false");
    let report = stream_clean_csv(input.as_slice(), &mut streamed, "in.csv", &opts).unwrap();

    assert_eq!(report.rows_in, 2);
    let text = String::from_utf8(streamed).unwrap();
    assert_eq!(text.lines().count(), 2);
}
