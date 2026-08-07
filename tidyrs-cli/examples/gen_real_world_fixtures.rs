//! Generates larger, more realistic fixtures under fixtures/real_world/
//! for the scenario test suite (tidyrs-cli/tests/real_world_scenarios.rs).
//! Unlike the small hand-written fixtures under fixtures/<format>/ (which
//! each isolate one specific behavior), these mix several kinds of mess
//! in the same file the way an actual export from a real system would,
//! at a size (100+ rows) where "it happens to work on 4 rows" isn't
//! enough. Deterministic (fixed seed) so regenerating produces the same
//! bytes. Run with:
//! `cargo run -p tidyrs-cli --example gen_real_world_fixtures`

use rust_xlsxwriter::{Format, Workbook};
use std::fmt::Write as _;
use std::io::Write;

/// Tiny deterministic PRNG (xorshift64) so this has no extra dependency
/// and always produces the same fixtures given the same seed.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() % 1_000_000) as f64 / 1_000_000.0
    }
    fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.next_u64() as u32) % (hi - lo)
    }
    fn choice<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next_u64() as usize % items.len()]
    }
}

fn gen_sales_csv(rng: &mut Rng) -> String {
    let customers = [
        "Alice Martin",
        "Bob Nguyen",
        "Charlotte Dubois",
        "David Kim",
        "Elena Rossi",
        "Francois Petit",
        "Jose Garcia",
        "Sohngen GmbH",
        "Zhang Wei",
        "Aicha Diallo",
        "O'Brien & Sons",
        "Muller AG",
        "Nakamura Corp",
        "Silva Ltda",
        "Kowalski sp. z o.o.",
    ];
    let statuses = ["paid", "pending", "refunded", "cancelled", "paid", "paid", "pending"];
    let currencies = ["EUR", "USD", "GBP"];
    let notes_pool = [
        "gift wrap requested",
        "VIP customer",
        "address changed mid-transit",
        "partial refund issued",
        "flagged for review",
    ];

    let mut out = String::new();
    out.push_str("order_id,customer_name,order_date,amount,currency,status,notes\n");

    let mut row_id = 1000u32;
    for _ in 0..130 {
        row_id += 1;
        let cust = rng.choice(&customers);
        let (y, m, d) = (2025, rng.range(1, 13), rng.range(1, 29));
        let date_str = match rng.range(0, 3) {
            0 => format!("{y:04}-{m:02}-{d:02}"),
            1 => format!("{d:02}/{m:02}/{y:04}"),
            _ => format!("{d:02}-{m:02}-{y:04}"),
        };
        let amount = 9.5 + rng.next_f64() * 4990.49;
        let messiness = rng.next_f64();
        let amount_str = if messiness < 0.08 {
            format!("${amount:.2}")
        } else if messiness < 0.14 {
            format!("{:.0} {:02}", (amount / 1.0).floor(), ((amount * 100.0) as u64) % 100)
        } else {
            format!("{amount:.2}")
        };
        let currency = rng.choice(&currencies);
        let status = rng.choice(&statuses);
        let notes: &str = if rng.next_f64() < 0.1 { rng.choice(&notes_pool) } else { "" };

        // Occasional blank line, like a spreadsheet-to-CSV export artifact.
        if rng.next_f64() < 0.03 {
            out.push('\n');
        }

        let mut fields = vec![
            row_id.to_string(),
            cust.to_string(),
            date_str,
            amount_str,
            currency.to_string(),
            status.to_string(),
            notes.to_string(),
        ];

        let r = rng.next_f64();
        if r < 0.05 {
            fields.pop(); // ragged: missing trailing field
        } else if r < 0.08 {
            fields.push("EXTRA".to_string()); // ragged: stray extra field
        }

        let rendered: Vec<String> = fields
            .iter()
            .map(|f| if f.contains(',') { format!("\"{f}\"") } else { f.clone() })
            .collect();
        writeln!(out, "{}", rendered.join(",")).unwrap();
    }
    out
}

fn gen_orders_json(rng: &mut Rng) -> String {
    let skus = ["A100", "A101", "B200", "B201", "C300", "C301", "D400"];
    let customers = ["Alice Martin", "Bob Nguyen", "Charlotte Dubois", "David Kim", "Elena Rossi"];
    let mut out = String::from("[\n");
    for i in 0..40 {
        let order_id = 5000 + i;
        let customer = rng.choice(&customers);
        let item_count = rng.range(1, 4);
        let mut items = Vec::new();
        for _ in 0..item_count {
            let sku = rng.choice(&skus);
            let qty = rng.range(1, 6);
            items.push(format!("{{\"sku\": \"{sku}\", \"qty\": {qty}}}"));
        }
        // Inconsistent optional field across records: sometimes a
        // shipping object, sometimes just a string, sometimes absent —
        // exactly the kind of real-world API drift tidyrs-json has to
        // survive without crashing.
        let shipping = match rng.range(0, 3) {
            0 => format!(
                ", \"shipping\": {{\"method\": \"express\", \"cost\": {:.2}}}",
                4.0 + rng.next_f64() * 20.0
            ),
            1 => ", \"shipping\": \"standard\"".to_string(),
            _ => String::new(),
        };
        let discount = if rng.next_f64() < 0.2 {
            format!(", \"discount_pct\": {}", rng.range(5, 30))
        } else {
            String::new()
        };

        writeln!(
            out,
            "  {{\"order_id\": {order_id}, \"customer\": \"{customer}\", \"items\": [{}]{shipping}{discount}}}{}",
            items.join(", "),
            if i < 39 { "," } else { "" }
        )
        .unwrap();
    }
    out.push_str("]\n");
    out
}

fn gen_server_log(rng: &mut Rng) -> String {
    let levels = ["INFO", "WARN", "ERROR", "INFO", "INFO", "DEBUG"];
    let messages = [
        "request completed",
        "connection pool exhausted, retrying",
        "cache miss for key",
        "upstream timeout after 30s",
        "user session expired",
        "background job finished",
        "rate limit exceeded for client",
    ];
    let mut out = String::new();
    for i in 0..80 {
        let h = 8 + (i / 20) % 12;
        let m = (i * 3) % 60;
        let s = (i * 7) % 60;
        let level = rng.choice(&levels);
        let msg = rng.choice(&messages);
        writeln!(
            out,
            "2026-01-{:02} {h:02}:{m:02}:{s:02} {level:<5} {msg} (req_id={:08x})",
            3 + (i % 20),
            rng.next_u64() as u32
        )
        .unwrap();
    }
    out
}

fn gen_accounts_yaml(rng: &mut Rng) -> String {
    let names = [
        "Alice Martin",
        "Bob Nguyen",
        "Charlotte Dubois",
        "David Kim",
        "Elena Rossi",
        "Francois Petit",
        "Jose Garcia",
        "Zhang Wei",
        "Aicha Diallo",
        "Muller AG",
    ];
    let plans = ["free", "pro", "enterprise"];
    let tags_pool = ["beta", "vip", "trial", "flagged", "partner"];

    let mut out = String::new();
    for i in 0..45u32 {
        let id = 9000 + i;
        let name = rng.choice(&names);
        let plan = rng.choice(&plans);
        let active = rng.next_f64() < 0.85;
        writeln!(out, "- id: {id}").unwrap();
        writeln!(out, "  name: {name}").unwrap();
        writeln!(out, "  plan: {plan}").unwrap();
        writeln!(out, "  active: {active}").unwrap();

        // Inconsistent optional field across records, same real-world
        // shape as gen_orders_json's "shipping" field: sometimes a
        // nested mapping, sometimes a plain scalar, sometimes absent
        // entirely — exactly what tidyrs-json's flattening has to
        // survive without crashing, now exercised through the YAML path.
        match rng.range(0, 3) {
            0 => {
                let amount = 9.99 + rng.next_f64() * 490.0;
                writeln!(out, "  billing:").unwrap();
                writeln!(out, "    method: card").unwrap();
                writeln!(out, "    amount: {amount:.2}").unwrap();
            }
            1 => writeln!(out, "  billing: invoiced").unwrap(),
            _ => {}
        }

        if rng.next_f64() < 0.4 {
            let tag_count = rng.range(1, 3);
            let tags: Vec<&str> = (0..tag_count).map(|_| *rng.choice(&tags_pool)).collect();
            writeln!(out, "  tags: [{}]", tags.join(", ")).unwrap();
        }
    }
    out
}

fn gen_services_ini(rng: &mut Rng) -> String {
    let mut out = String::new();
    out.push_str("; Service connection profiles, one per deployment environment\n");
    let environments = [
        ("dev", 5432, false),
        ("qa", 5432, false),
        ("staging", 5433, true),
        ("production", 5433, true),
    ];
    for (env, port, has_ssl) in environments {
        writeln!(out, "[{env}]").unwrap();
        writeln!(out, "host = {env}-db.internal.example.com").unwrap();
        writeln!(out, "port = {port}").unwrap();
        writeln!(out, "user = svc_{env}").unwrap();
        // "qa" deliberately omits timeout, like a real config that was
        // never fully filled in for a lower environment — one row's
        // missing key must come through as a real gap (Null), not force
        // every other row's value to disappear too.
        if env != "qa" {
            writeln!(out, "timeout = {}", rng.range(15, 60)).unwrap();
        }
        if has_ssl {
            writeln!(out, "ssl = true").unwrap();
        }
        out.push('\n');
    }
    out
}

fn gen_deploy_env(rng: &mut Rng) -> String {
    let mut out = String::new();
    out.push_str("# Deployment environment, sourced directly by the app's start script\n");
    writeln!(
        out,
        "export DATABASE_URL=postgres://svc_production:{:x}@production-db.internal.example.com:5433/app",
        rng.next_u64()
    )
    .unwrap();
    writeln!(out, "export REDIS_URL=redis://cache.internal.example.com:6379/0").unwrap();
    writeln!(out, "API_KEY=\"{:016x}\"", rng.next_u64()).unwrap();
    out.push_str("DEBUG=false\n");
    out.push_str("LOG_LEVEL=info\n");
    writeln!(out, "MAX_WORKERS={}", rng.range(4, 32)).unwrap();
    out.push_str("# Feature flags\n");
    out.push_str("export FEATURE_NEW_CHECKOUT=true\n");
    out.push_str("FEATURE_BETA_DASHBOARD=false\n");
    out.push_str("SUPPORT_EMAIL='support@example.com'\n");
    out
}

fn gen_shop_sqlite(path: &std::path::Path, rng: &mut Rng) {
    let _ = std::fs::remove_file(path);
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT, country TEXT, joined_date TEXT);
         CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price REAL NOT NULL, in_stock INTEGER);
         CREATE TABLE orders (order_id INTEGER PRIMARY KEY, customer_id INTEGER, product_id INTEGER, qty INTEGER, total REAL, order_date TEXT);",
    )
    .unwrap();

    let names = [
        "Alice Martin",
        "Bob Nguyen",
        "Charlotte Dubois",
        "David Kim",
        "Elena Rossi",
        "Francois Petit",
        "Jose Garcia",
        "Zhang Wei",
    ];
    let countries = ["FR", "DE", "US", "GB", "JP"];
    for i in 0..30u32 {
        let id = 1 + i;
        let name = rng.choice(&names);
        // Realistic optional-field gap: not every customer record has a
        // verified email on file.
        let email: Option<String> = if rng.next_f64() < 0.85 {
            Some(format!("{}@example.com", name.to_lowercase().replace(' ', ".")))
        } else {
            None
        };
        let country = rng.choice(&countries);
        conn.execute(
            "INSERT INTO customers (id, name, email, country, joined_date) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, name, email, country, format!("2025-{:02}-{:02}", rng.range(1, 13), rng.range(1, 29))],
        )
        .unwrap();
    }

    let product_names = ["Widget", "Gadget", "Thingamajig", "Doohickey", "Contraption", "Gizmo", "Sprocket"];
    for i in 0..15u32 {
        let id = 1 + i;
        let name = rng.choice(&product_names);
        let price = 4.99 + rng.next_f64() * 195.0;
        conn.execute(
            "INSERT INTO products (id, name, price, in_stock) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, format!("{name} #{id}"), price, rng.next_f64() < 0.9],
        )
        .unwrap();
    }

    for i in 0..60u32 {
        let order_id = 5000 + i;
        let customer_id = rng.range(1, 31);
        let product_id = rng.range(1, 16);
        let qty = rng.range(1, 6);
        let price = conn
            .query_row("SELECT price FROM products WHERE id = ?1", [product_id], |row| row.get::<_, f64>(0))
            .unwrap();
        let total = price * qty as f64;
        conn.execute(
            "INSERT INTO orders (order_id, customer_id, product_id, qty, total, order_date) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                order_id,
                customer_id,
                product_id,
                qty,
                total,
                format!("2025-{:02}-{:02}", rng.range(1, 13), rng.range(1, 29))
            ],
        )
        .unwrap();
    }
}

fn gen_financial_report_xlsx(path: &std::path::Path) {
    let mut wb = Workbook::new();
    let fmt = Format::new();

    // Sheet 1: junk title row, a clean header, several data rows, and a
    // TOTAL footer row that (realistically) has more than one populated
    // cell, so it survives as a data row rather than getting trimmed as
    // junk — that's exactly what a real spreadsheet's totals row looks
    // like, and it's worth the scenario suite documenting that as
    // expected behavior rather than a bug.
    let summary = wb.add_worksheet().set_name("Summary").unwrap();
    summary.write_string(0, 0, "Q4 2025 Financial Report - Confidential").unwrap();
    summary.write_string(1, 0, "region").unwrap();
    summary.write_string(1, 1, "q4_revenue").unwrap();
    summary.write_string(1, 2, "q4_expenses").unwrap();
    summary.write_string(1, 3, "net").unwrap();
    let regions = [
        ("North", 482_000.0, 310_500.0),
        ("South", 355_200.0, 290_100.0),
        ("East", 601_750.0, 420_000.0),
        ("West", 274_900.0, 198_300.0),
        ("Central", 390_000.0, 275_600.0),
    ];
    for (i, (name, rev, exp)) in regions.iter().enumerate() {
        let r = (2 + i) as u32;
        summary.write_string(r, 0, *name).unwrap();
        summary.write_number(r, 1, *rev).unwrap();
        summary.write_number(r, 2, *exp).unwrap();
        summary.write_number(r, 3, rev - exp).unwrap();
    }
    let total_row = 2 + regions.len() as u32;
    let total_rev: f64 = regions.iter().map(|(_, r, _)| r).sum();
    let total_exp: f64 = regions.iter().map(|(_, _, e)| e).sum();
    summary.write_string(total_row, 0, "TOTAL").unwrap();
    summary.write_number(total_row, 1, total_rev).unwrap();
    summary.write_number(total_row, 2, total_exp).unwrap();
    summary.write_number(total_row, 3, total_rev - total_exp).unwrap();

    // Sheet 2: a "region" column merged across each group's rows, the way
    // a manually-grouped Excel report typically looks — different shape
    // entirely from Sheet 1, on purpose (tidyrs-xlsx normalizes each
    // sheet independently).
    let detail = wb.add_worksheet().set_name("Regional Detail").unwrap();
    detail.write_string(0, 0, "region").unwrap();
    detail.write_string(0, 1, "city").unwrap();
    detail.write_string(0, 2, "rep").unwrap();
    detail.write_string(0, 3, "sales").unwrap();
    type RegionRow<'a> = (&'a str, &'a str, f64);
    let groups: [(&str, &[RegionRow]); 3] = [
        (
            "North",
            &[
                ("Lille", "Alice Martin", 82000.0),
                ("Lyon", "Bob Nguyen", 91000.0),
                ("Paris", "Charlotte Dubois", 130500.0),
            ],
        ),
        (
            "South",
            &[
                ("Marseille", "David Kim", 120200.0),
                ("Nice", "Elena Rossi", 95000.0),
                ("Toulouse", "Francois Petit", 140000.0),
            ],
        ),
        (
            "East",
            &[
                ("Strasbourg", "Jose Garcia", 210000.0),
                ("Metz", "Zhang Wei", 180000.0),
                ("Nancy", "Aicha Diallo", 211750.0),
            ],
        ),
    ];
    let mut r = 1u32;
    for (region, rows) in groups.iter() {
        let start = r;
        for (city, rep, sales) in rows.iter() {
            detail.write_string(r, 1, *city).unwrap();
            detail.write_string(r, 2, *rep).unwrap();
            detail.write_number(r, 3, *sales).unwrap();
            r += 1;
        }
        let end = r - 1;
        if end > start {
            detail.merge_range(start, 0, end, 0, region, &fmt).unwrap();
        } else {
            detail.write_string(start, 0, *region).unwrap();
        }
    }

    // Sheet 3: single free-text column — not really "tabular" at all, a
    // realistic "someone left a Notes tab in the workbook" case that
    // should still parse cleanly as a one-column table, not error out.
    let notes = wb.add_worksheet().set_name("Notes").unwrap();
    notes.write_string(0, 0, "note").unwrap();
    let free_text = [
        "Q4 numbers pending final audit sign-off.",
        "West region affected by warehouse relocation in November.",
        "Central region added mid-quarter, partial data only.",
        "Contact finance@example.com for questions.",
    ];
    for (i, t) in free_text.iter().enumerate() {
        notes.write_string((1 + i) as u32, 0, *t).unwrap();
    }

    wb.save(path).unwrap();
}

fn main() {
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/real_world");
    std::fs::create_dir_all(&out_dir).unwrap();

    let mut rng = Rng::new(0xC0FFEE_u64);
    std::fs::File::create(out_dir.join("sales_export_messy.csv"))
        .unwrap()
        .write_all(gen_sales_csv(&mut rng).as_bytes())
        .unwrap();

    let mut rng = Rng::new(0xBADA55_u64);
    std::fs::File::create(out_dir.join("orders_nested.json"))
        .unwrap()
        .write_all(gen_orders_json(&mut rng).as_bytes())
        .unwrap();

    let mut rng = Rng::new(0x1337_u64);
    std::fs::File::create(out_dir.join("server_activity.log"))
        .unwrap()
        .write_all(gen_server_log(&mut rng).as_bytes())
        .unwrap();

    gen_financial_report_xlsx(&out_dir.join("q4_financial_report.xlsx"));

    let mut rng = Rng::new(0xACC0_u64);
    std::fs::File::create(out_dir.join("accounts_export.yaml"))
        .unwrap()
        .write_all(gen_accounts_yaml(&mut rng).as_bytes())
        .unwrap();

    let mut rng = Rng::new(0x5EC0_u64);
    std::fs::File::create(out_dir.join("services.ini"))
        .unwrap()
        .write_all(gen_services_ini(&mut rng).as_bytes())
        .unwrap();

    let mut rng = Rng::new(0xDEC1_u64);
    std::fs::File::create(out_dir.join("deploy.env"))
        .unwrap()
        .write_all(gen_deploy_env(&mut rng).as_bytes())
        .unwrap();

    let mut rng = Rng::new(0x540F_u64);
    gen_shop_sqlite(&out_dir.join("shop.db"), &mut rng);

    println!("wrote fixtures to {}", out_dir.display());
}
