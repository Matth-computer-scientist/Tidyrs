//! Generates the .xlsx fixtures committed under fixtures/xlsx/. Run with
//! `cargo run -p tidyrs-xlsx --example gen_fixtures_xlsx` whenever the fixtures
//! need to be regenerated; the resulting files are checked into the repo
//! so tests don't depend on re-running this.

use rust_xlsxwriter::{Format, Workbook};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/xlsx");
    std::fs::create_dir_all(&out_dir)?;

    // 1. Junk title row + footer row + a vertically merged "team" column.
    {
        let mut wb = Workbook::new();
        let fmt = Format::new();
        let ws = wb.add_worksheet();
        ws.write_string(0, 0, "Quarterly Report - Confidential")?;
        ws.write_string(1, 0, "name")?;
        ws.write_string(1, 1, "score")?;
        ws.write_string(1, 2, "team")?;
        ws.write_string(2, 0, "Alice")?;
        ws.write_number(2, 1, 91)?;
        ws.merge_range(2, 2, 4, 2, "Blue", &fmt)?;
        ws.write_string(3, 0, "Bob")?;
        ws.write_number(3, 1, 77)?;
        ws.write_string(4, 0, "Carla")?;
        ws.write_number(4, 1, 88)?;
        ws.write_string(5, 0, "Generated automatically - do not edit")?;
        wb.save(out_dir.join("junk_rows_and_merged_cells.xlsx"))?;
    }

    // 2. Multi-sheet workbook where each sheet has a different shape.
    {
        let mut wb = Workbook::new();
        let ws1 = wb.add_worksheet().set_name("People")?;
        ws1.write_string(0, 0, "name")?;
        ws1.write_string(0, 1, "age")?;
        ws1.write_string(1, 0, "Dana")?;
        ws1.write_number(1, 1, 34)?;
        ws1.write_string(2, 0, "Eli")?;
        ws1.write_number(2, 1, 29)?;

        let ws2 = wb.add_worksheet().set_name("Orders")?;
        ws2.write_string(0, 0, "product")?;
        ws2.write_string(0, 1, "price")?;
        ws2.write_string(0, 2, "qty")?;
        ws2.write_string(1, 0, "Widget")?;
        ws2.write_number(1, 1, 9.99)?;
        ws2.write_number(1, 2, 3)?;
        ws2.write_string(2, 0, "Gadget")?;
        ws2.write_number(2, 1, 14.5)?;
        ws2.write_number(2, 2, 1)?;
        wb.save(out_dir.join("multi_sheet_different_shapes.xlsx"))?;
    }

    // 3. Several blank/title rows before the real header.
    {
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet();
        ws.write_string(0, 0, "Internal Use Only")?;
        // row 1 left entirely blank on purpose
        ws.write_string(2, 0, "sku")?;
        ws.write_string(2, 1, "description")?;
        ws.write_string(2, 2, "in_stock")?;
        ws.write_string(3, 0, "A1")?;
        ws.write_string(3, 1, "Blue Widget")?;
        ws.write_boolean(3, 2, true)?;
        ws.write_string(4, 0, "A2")?;
        ws.write_string(4, 1, "Red Widget")?;
        ws.write_boolean(4, 2, false)?;
        wb.save(out_dir.join("leading_blank_and_title_rows.xlsx"))?;
    }

    // 4. Two separate merges in the same column, with an unmerged blank
    // cell between them. A naive column forward-fill heuristic would leak
    // "Blue" into that blank cell (it's just "empty, so inherit above");
    // exact merge-region boundaries must leave it Null and only fill the
    // second, genuinely merged region with "Red".
    {
        let mut wb = Workbook::new();
        let fmt = Format::new();
        let ws = wb.add_worksheet();
        ws.write_string(0, 0, "name")?;
        ws.write_string(0, 1, "team")?;
        ws.write_string(1, 0, "Alice")?;
        ws.merge_range(1, 1, 2, 1, "Blue", &fmt)?;
        ws.write_string(2, 0, "Bob")?;
        ws.write_string(3, 0, "Carla")?;
        // row 3 (Carla), col 1 intentionally left blank - NOT merged
        ws.write_string(4, 0, "Dave")?;
        ws.merge_range(4, 1, 5, 1, "Red", &fmt)?;
        ws.write_string(5, 0, "Eve")?;
        wb.save(out_dir.join("two_merges_with_gap_between.xlsx"))?;
    }

    println!("wrote fixtures to {}", out_dir.display());
    Ok(())
}
