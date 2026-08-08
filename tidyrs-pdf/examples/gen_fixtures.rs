//! Generates the .pdf fixtures committed under fixtures/pdf/. Run with
//! `cargo run -p tidyrs-pdf --example gen_fixtures_pdf` to regenerate. Uses a
//! monospaced (Courier) built-in font and pre-padded text lines so the
//! whitespace-alignment column heuristic in tidyrs-pdf has a realistic
//! chance of reconstructing the table — real-world PDFs from proportional
//! fonts are much harder, which is exactly why this format stays
//! experimental.

use printpdf::{BuiltinFont, Mm, PdfDocument};
use std::fs::File;
use std::io::BufWriter;

fn write_lines(path: &std::path::Path, lines: &[&str]) {
    let (doc, page1, layer1) = PdfDocument::new("tidyloom fixture", Mm(210.0), Mm(297.0), "Layer 1");
    let layer = doc.get_page(page1).get_layer(layer1);
    let font = doc.add_builtin_font(BuiltinFont::Courier).unwrap();

    let mut y = 280.0;
    for line in lines {
        layer.use_text(*line, 11.0, Mm(15.0), Mm(y), &font);
        y -= 8.0;
    }

    doc.save(&mut BufWriter::new(File::create(path).unwrap())).unwrap();
}

/// Writes each field at its own fixed x/y position with a proportional
/// font (Helvetica) — the realistic way a real invoice/report generator
/// lays out a table (one text-show call per cell, not one string padded
/// with spaces to a monospace grid). This is exactly the case the old
/// text-flow-heuristic approach documented as broken, and what the
/// glyph-position extraction in `glyphs.rs` is meant to fix.
fn write_columns(path: &std::path::Path, header: &[&str], rows: &[[&str; 3]], col_x_mm: [f32; 3]) {
    let (doc, page1, layer1) = PdfDocument::new("tidyloom fixture", Mm(210.0), Mm(297.0), "Layer 1");
    let layer = doc.get_page(page1).get_layer(layer1);
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();

    let mut y = 280.0;
    for (col, text) in header.iter().enumerate() {
        layer.use_text(*text, 12.0, Mm(col_x_mm[col]), Mm(y), &font);
    }
    y -= 8.0;
    for row in rows {
        for (col, text) in row.iter().enumerate() {
            layer.use_text(*text, 11.0, Mm(col_x_mm[col]), Mm(y), &font);
        }
        y -= 8.0;
    }

    doc.save(&mut BufWriter::new(File::create(path).unwrap())).unwrap();
}

/// Like `write_columns` but for an arbitrary column count and rows with
/// some cells legitimately empty (skipped entirely — no glyph placed at
/// all, the way a real generator renders "no value" rather than an empty
/// string). This is the shape that exposed a real header/row-loss bug in
/// `find_header_offset`: a title line above a table whose data rows
/// aren't perfectly uniform (some optional fields blank) — see
/// `title_line_above_ragged_data_does_not_lose_rows_or_the_header` in
/// tests/fixtures.rs.
fn write_columns_n(path: &std::path::Path, title: &str, header: &[&str], rows: &[Vec<&str>], col_x_mm: &[f32]) {
    let (doc, page1, layer1) = PdfDocument::new("tidyloom fixture", Mm(210.0), Mm(297.0), "Layer 1");
    let layer = doc.get_page(page1).get_layer(layer1);
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();

    let mut y = 280.0;
    layer.use_text(title, 14.0, Mm(15.0), Mm(y), &font);
    y -= 10.0;
    for (col, text) in header.iter().enumerate() {
        layer.use_text(*text, 12.0, Mm(col_x_mm[col]), Mm(y), &font);
    }
    y -= 8.0;
    for row in rows {
        for (col, text) in row.iter().enumerate() {
            if !text.is_empty() {
                layer.use_text(*text, 11.0, Mm(col_x_mm[col]), Mm(y), &font);
            }
        }
        y -= 8.0;
    }

    doc.save(&mut BufWriter::new(File::create(path).unwrap())).unwrap();
}

/// Like `write_lines` but splits the given lines across two pages at
/// `split_at`, both using the same left margin/y-start layout. Used to
/// reproduce the page-boundary row-merging bug in `glyphs.rs`: each
/// page's own coordinate flip is relative to that page's own media box,
/// so page 2's rows land at nearly the same (x, y) as page 1's — without
/// tracking which page a glyph came from, row-clustering had no way to
/// tell those apart and would merge/interleave unrelated rows from
/// different pages into one.
fn write_two_page_table(path: &std::path::Path, lines: &[&str], split_at: usize) {
    let (doc, page1, layer1) = PdfDocument::new("tidyloom fixture", Mm(210.0), Mm(297.0), "Layer 1");
    let font = doc.add_builtin_font(BuiltinFont::Courier).unwrap();

    let layer = doc.get_page(page1).get_layer(layer1);
    let mut y = 280.0;
    for line in &lines[..split_at] {
        layer.use_text(*line, 11.0, Mm(15.0), Mm(y), &font);
        y -= 8.0;
    }

    let (page2, layer2i) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
    let layer2 = doc.get_page(page2).get_layer(layer2i);
    let mut y2 = 280.0;
    for line in &lines[split_at..] {
        layer2.use_text(*line, 11.0, Mm(15.0), Mm(y2), &font);
        y2 -= 8.0;
    }

    doc.save(&mut BufWriter::new(File::create(path).unwrap())).unwrap();
}

fn main() {
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/pdf");
    std::fs::create_dir_all(&out_dir).unwrap();

    write_lines(
        &out_dir.join("simple_table.pdf"),
        &[
            "name       age  city",
            "Alice      30   Paris",
            "Bob        41   Lyon",
            "Charlotte  25   Marseille",
        ],
    );

    write_lines(
        &out_dir.join("table_with_title.pdf"),
        &[
            "Quarterly Sales Report",
            "region     units   revenue",
            "North      120     4500.50",
            "South      95      3120.00",
            "East       210     8899.99",
        ],
    );

    write_lines(
        &out_dir.join("product_table.pdf"),
        &[
            "sku    description       price",
            "A100   Blue Widget       9.99",
            "A101   Red Widget        14.50",
            "A102   Green Widget      3.25",
        ],
    );

    write_columns(
        &out_dir.join("proportional_font_per_field_table.pdf"),
        &["name", "age", "city"],
        &[["Alice", "30", "Paris"], ["Bob", "41", "Lyon"], ["Charlotte", "25", "Marseille"]],
        [15.0, 60.0, 100.0],
    );

    write_columns_n(
        &out_dir.join("title_with_ragged_data.pdf"),
        "Rapport Ventes - Janvier 2026",
        &["SKU", "description", "category", "qty", "price"],
        &[
            vec!["SKU-001", "Casque Audio Bluetooth Pro", "Electronique", "8", "89.90"],
            vec!["SKU-002", "Cable USB-C 2m", "Accessoires", "", "5.50"],
            vec!["SKU-003", "Souris Gamer RGB", "", "12", "45"],
            vec!["SKU-004", "Chaise de Bureau Ergonomique", "Mobilier", "3", "249"],
        ],
        &[15.0, 45.0, 110.0, 145.0, 165.0],
    );

    // Same multi-word title, but over a "clean" table with no ragged/
    // empty cells — a distinct counter-example from the one above, found
    // via a follow-up external QA report: the title's own internal word
    // gaps ("Rapport" / "Ventes" / "-" / "Janvier 2026") can coincidentally
    // subdivide a region the real table only sees as one wide gap,
    // producing *more* apparent columns with the title included than
    // without — the opposite of what the title-skip heuristic assumes.
    // See the `find_header_offset` docs for why this is accepted as a
    // known limitation rather than patched: every fix attempted for it
    // broke other, more common cases (a real header being mistaken for a
    // title). Kept as a fixture specifically to pin down and prove the
    // *bounded* nature of the failure — the title survives as extra
    // ambiguous columns, not silently dropped data.
    write_columns_n(
        &out_dir.join("title_with_no_ragged_data.pdf"),
        "Rapport Ventes - Janvier 2026",
        &["region", "units", "revenue"],
        &[
            vec!["North", "120", "4500.50"],
            vec!["South", "95", "3120.00"],
            vec!["East", "210", "8899.99"],
        ],
        &[15.0, 60.0, 100.0],
    );

    // Found via external QA testing: a monospaced table whose data cells
    // contain an internal space ("Widget A") and whose numeric columns are
    // right-aligned within wide fields. Excluding the real header line
    // ("product qty amount") produces *more* inferred columns than
    // including it, because "Widget A"/"Widget B"/"Widget C" all happen to
    // share a whitespace gap at the same position that the header text
    // doesn't share — the mirror image of the title-line problem above
    // (there, including a junk line created spurious columns; here,
    // excluding the real header does). find_header_offset's "more columns
    // wins" rule picks the wrong side, so the header is lost and the first
    // data row is misread as the header. See
    // `a_data_cell_that_looks_like_two_words_can_still_cost_the_header` in
    // tests/fixtures.rs for the accepted, bounded nature of this failure.
    write_lines(
        &out_dir.join("right_aligned_numbers.pdf"),
        &[
            "product           qty     amount",
            "Widget A            8       89.90",
            "Widget B           12       45.00",
            "Widget C            3      249.00",
        ],
    );

    // Found via external QA testing: a genuine multi-page table used to
    // corrupt both its last page-1 rows and first page-2 rows, because
    // `group_into_rows` clustered purely on Y position with no concept of
    // page boundaries — see `write_two_page_table`'s docs and
    // `glyphs::group_into_rows` for the fix. Header on page 1 only, data
    // continuing onto page 2 with no repeated header — the realistic
    // shape for a multi-page report/invoice.
    write_two_page_table(
        &out_dir.join("multi_page_table.pdf"),
        &[
            "sku    description       price",
            "A100   Blue Widget       9.99",
            "A101   Red Widget        14.50",
            "A102   Green Widget      3.25",
            "A103   Yellow Widget     7.00",
        ],
        3,
    );

    println!("wrote fixtures to {}", out_dir.display());
}
