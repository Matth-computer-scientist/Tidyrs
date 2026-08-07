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

    println!("wrote fixtures to {}", out_dir.display());
}
