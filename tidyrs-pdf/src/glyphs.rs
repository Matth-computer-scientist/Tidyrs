//! Real glyph-position extraction, replacing the old approach of running
//! whitespace-alignment inference directly on `pdf-extract`'s flattened
//! text output. That approach only worked because our own test fixtures
//! used a monospaced font (Courier) — a real-world PDF with a proportional
//! font (Helvetica, Times, ...) has no reason to line up character-for-
//! character even when its columns are genuinely aligned in points/mm,
//! which is what actually breaks table reconstruction on real documents.
//!
//! `pdf-extract` exposes a lower-level `OutputDev` trait that receives
//! each glyph's real transform (hence real x/y position in page space) as
//! the page is parsed. We collect every glyph's position and font size,
//! then group them into visual rows ourselves by clustering on Y
//! coordinate. Note this deliberately does *not* rely on `OutputDev`'s
//! `end_line()` callback: that fires on every `Tm`/`Td`/`TD`/`T*` text-
//! positioning operator, which means a table built from one separate
//! text-show call per *field* (the common case for real generated PDFs —
//! invoices, reports) would fragment a single visual row into many
//! spurious "lines". Y-clustering is robust to that: it doesn't care how
//! many content-stream operators contributed to a row, only where the
//! glyphs actually ended up on the page.
//!
//! Once rows are grouped, each row's glyph positions are converted into a
//! "virtual monospace" string — spacing measured in real character-pitch
//! units rather than character counts — so the existing whitespace-
//! alignment column inference in `lib.rs` (built around counting
//! characters) works unchanged on geometrically accurate spacing
//! regardless of the source font.

use pdf_extract::{ColorSpace, Document, MediaBox, OutputDev, OutputError, Path as PdfPath, Transform};

struct Glyph {
    x: f64,
    y: f64,
    font_size: f64,
    ch: char,
}

struct GlyphCollector {
    flip_ctm: Transform,
    glyphs: Vec<Glyph>,
}

impl GlyphCollector {
    fn new() -> Self {
        Self {
            flip_ctm: Transform::identity(),
            glyphs: Vec::new(),
        }
    }
}

impl OutputDev for GlyphCollector {
    fn begin_page(&mut self, _page_num: u32, media_box: &MediaBox, _art_box: Option<(f64, f64, f64, f64)>) -> Result<(), OutputError> {
        // Same flip used internally by pdf-extract's own PlainTextOutput:
        // PDF space is bottom-up, we want top-down reading order.
        self.flip_ctm = Transform::row_major(1., 0., 0., -1., 0., media_box.ury - media_box.lly);
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn output_character(&mut self, trm: &Transform, _width: f64, _spacing: f64, font_size: f64, char: &str) -> Result<(), OutputError> {
        let position = trm.post_transform(&self.flip_ctm);
        let (x, y) = (position.m31, position.m32);
        // Good enough as a row-clustering tolerance basis without pulling
        // in `euclid` directly to compute the fully CTM-transformed size.
        let effective_font_size = font_size.abs().max(1.0);
        for c in char.chars() {
            self.glyphs.push(Glyph {
                x,
                y,
                font_size: effective_font_size,
                ch: c,
            });
        }
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        // Deliberately not used for row grouping — see module docs.
        Ok(())
    }

    fn stroke(&mut self, _ctm: &Transform, _colorspace: &ColorSpace, _color: &[f64], _path: &PdfPath) -> Result<(), OutputError> {
        Ok(())
    }

    fn fill(&mut self, _ctm: &Transform, _colorspace: &ColorSpace, _color: &[f64], _path: &PdfPath) -> Result<(), OutputError> {
        Ok(())
    }
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(values[values.len() / 2])
}

/// Groups glyphs into visual rows by clustering on Y position: a glyph
/// starts a new row when it's further from the current row's reference Y
/// than half the document's typical font size (a tolerance that's
/// forgiving of small baseline jitter within one printed line but still
/// separates genuinely different lines).
fn group_into_rows(mut glyphs: Vec<Glyph>) -> Vec<Vec<Glyph>> {
    if glyphs.is_empty() {
        return vec![];
    }
    glyphs.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));

    let tolerance = median(glyphs.iter().map(|g| g.font_size).collect()).unwrap_or(10.0) * 0.5;

    let mut rows: Vec<Vec<Glyph>> = Vec::new();
    let mut current: Vec<Glyph> = Vec::new();
    let mut row_y = glyphs[0].y;

    for g in glyphs {
        if !current.is_empty() && (g.y - row_y).abs() > tolerance {
            current.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            rows.push(std::mem::take(&mut current));
        }
        if current.is_empty() {
            row_y = g.y;
        }
        current.push(g);
    }
    if !current.is_empty() {
        current.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        rows.push(current);
    }
    rows
}

/// Converts each row's glyph positions into a plain string where
/// horizontal spacing is expressed in whole "character cells" derived
/// from the document's own typical glyph pitch, instead of literal PDF
/// point coordinates.
fn rows_to_virtual_text(rows: Vec<Vec<Glyph>>) -> Vec<String> {
    let mut deltas = Vec::new();
    for row in &rows {
        for w in row.windows(2) {
            let d = w[1].x - w[0].x;
            if d > 0.01 {
                deltas.push(d);
            }
        }
    }
    let unit = median(deltas).unwrap_or(5.0).max(0.5);
    // Proportional fonts have wildly varying per-character advance widths
    // (an 'm' or 'w' can be 2-3x wider than an 'i' or 'l'), so the gap
    // after a single wide glyph can already exceed the *median* advance
    // used as `unit` — rounding naively there would insert a spurious
    // space in the middle of a word. Require a gap notably larger than
    // typical (not just "above the median") before treating it as a real
    // separator, so intra-word variation among wide characters never
    // trips it, while genuine word/column gaps (which are geometrically
    // much wider than any single glyph advance) still do.
    const SEPARATOR_THRESHOLD: f64 = 1.8;

    rows.into_iter()
        .map(|row| {
            let mut s = String::new();
            let mut prev_x: Option<f64> = None;
            for g in row {
                if let Some(px) = prev_x {
                    let gap = g.x - px;
                    if gap > unit * SEPARATOR_THRESHOLD {
                        let cells = (gap / unit).round().max(2.0) as usize;
                        for _ in 1..cells {
                            s.push(' ');
                        }
                    }
                }
                s.push(g.ch);
                prev_x = Some(g.x);
            }
            s
        })
        .collect()
}

/// Extracts each visual row of text as a virtual-monospace string with
/// geometrically accurate spacing (see module docs). Returns an error for
/// anything `pdf-extract`'s document loader itself rejects (e.g. not a
/// valid PDF).
pub fn extract_virtual_lines(bytes: &[u8]) -> Result<Vec<String>, String> {
    let doc = Document::load_mem(bytes).map_err(|e| e.to_string())?;
    let mut collector = GlyphCollector::new();
    pdf_extract::output_doc(&doc, &mut collector).map_err(|e| e.to_string())?;
    Ok(rows_to_virtual_text(group_into_rows(collector.glyphs)))
}
