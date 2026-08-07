//! Generates the .db fixtures committed under fixtures/sqlite/. Run with
//! `cargo run -p tidyrs-sqlite --example gen_fixtures` whenever the
//! fixtures need to be regenerated; the resulting files are checked into
//! the repo so tests don't depend on re-running this.

use rusqlite::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sqlite");
    std::fs::create_dir_all(&out_dir)?;

    // 1. Single-table database with a mix of column types, including NULL.
    {
        let path = out_dir.join("single_table.db");
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER, balance REAL, active INTEGER);
             INSERT INTO users (id, name, age, balance, active) VALUES
                (1, 'Alice', 30, 120.5, 1),
                (2, 'Bob', NULL, 0.0, 0),
                (3, 'Charlotte', 41, 999.99, 1);",
        )?;
        drop(conn);
    }

    // 2. Multi-table database (each table becomes its own TidyTable, like
    //    one sheet per workbook tab in tidyrs-xlsx).
    {
        let path = out_dir.join("multi_table.db");
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, country TEXT);
             INSERT INTO customers (id, name, country) VALUES (1, 'Acme Corp', 'FR'), (2, 'Globex', 'DE');

             CREATE TABLE orders (order_id INTEGER PRIMARY KEY, customer_id INTEGER, total REAL);
             INSERT INTO orders (order_id, customer_id, total) VALUES (1001, 1, 250.0), (1002, 2, 75.5), (1003, 1, 30.0);",
        )?;
        drop(conn);
    }

    println!("wrote fixtures to {}", out_dir.display());
    Ok(())
}
