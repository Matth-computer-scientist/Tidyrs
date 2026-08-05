//! Ad-hoc performance check for large inputs. Not a criterion benchmark
//! (kept dependency-free on purpose) — just wall-clock timing for CSV and
//! Excel parsing at increasing row counts, to put a number on the
//! documented "everything loads into memory" limitation instead of
//! leaving it as an unverified claim. Run with:
//! `cargo run -p tidyrs-cli --release --example bench_large_files`
//!
//! Methodology note: this generates synthetic files (uniform rows, no
//! real-world irregularity) on whatever machine runs it — it is not a
//! benchmark against a real-world corpus, and the numbers will vary by
//! hardware. It exists to catch regressions and support order-of-magnitude
//! claims ("CSV scales close to linearly"), not to be a precise SLA.

use rust_xlsxwriter::Workbook;
use std::io::Write;
use std::time::Instant;
use tidyrs_core::{ParseOptions, TidyParser};

fn generate_csv(path: &std::path::Path, rows: usize) {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    writeln!(f, "id,name,amount,category,active").unwrap();
    for i in 0..rows {
        writeln!(f, "{i},item_{i},{:.2},cat_{},{}", (i as f64) * 1.37, i % 12, i % 2 == 0).unwrap();
    }
}

fn generate_xlsx(path: &std::path::Path, rows: usize) {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, "id").unwrap();
    ws.write_string(0, 1, "name").unwrap();
    ws.write_string(0, 2, "amount").unwrap();
    for i in 0..rows {
        let r = (i + 1) as u32;
        ws.write_number(r, 0, i as f64).unwrap();
        ws.write_string(r, 1, format!("item_{i}")).unwrap();
        ws.write_number(r, 2, (i as f64) * 1.37).unwrap();
    }
    wb.save(path).unwrap();
}

fn time_it<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    println!("{label}: {:.2?}", start.elapsed());
    result
}

fn main() {
    let dir = std::env::temp_dir().join("tidyloom_bench");
    std::fs::create_dir_all(&dir).unwrap();

    for &rows in &[10_000usize, 100_000, 500_000] {
        println!("\n=== {rows} rows ===");

        let csv_path = dir.join(format!("bench_{rows}.csv"));
        time_it("  generate csv", || generate_csv(&csv_path, rows));
        let csv_bytes = std::fs::read(&csv_path).unwrap();
        println!("  csv file size: {:.1} MB", csv_bytes.len() as f64 / 1_000_000.0);

        let parser = tidyrs_csv::CsvParser::new();
        let outcome = time_it("  parse csv (in-memory)", || {
            parser.parse(&csv_bytes, "bench.csv", &ParseOptions::new()).unwrap()
        });
        assert_eq!(outcome.tables[0].rows.len(), rows);

        let stream_out_path = dir.join(format!("bench_{rows}_stream_out.csv"));
        time_it("  parse csv (--stream, bounded memory)", || {
            let input = std::fs::File::open(&csv_path).unwrap();
            let output = std::fs::File::create(&stream_out_path).unwrap();
            tidyrs_csv::stream_clean_csv(input, output, "bench.csv", &ParseOptions::new()).unwrap()
        });
        std::fs::remove_file(&stream_out_path).ok();
        std::fs::remove_file(&csv_path).ok();

        // Excel gets slow to *generate* well before it's interesting to
        // parse, and calamine loads the whole sheet into Vec<Vec<Data>>
        // regardless of size (see README "Performance notes") — cap it
        // lower than CSV so the bench finishes in reasonable time.
        if rows <= 100_000 {
            let xlsx_path = dir.join(format!("bench_{rows}.xlsx"));
            time_it("  generate xlsx", || generate_xlsx(&xlsx_path, rows));
            let xlsx_bytes = std::fs::read(&xlsx_path).unwrap();
            println!("  xlsx file size: {:.1} MB", xlsx_bytes.len() as f64 / 1_000_000.0);
            let parser = tidyrs_xlsx::XlsxParser::new();
            let outcome = time_it("  parse xlsx", || parser.parse(&xlsx_bytes, "bench.xlsx", &ParseOptions::new()).unwrap());
            assert_eq!(outcome.tables[0].rows.len(), rows);
            std::fs::remove_file(&xlsx_path).ok();
        }
    }

    std::fs::remove_dir_all(&dir).ok();
}
