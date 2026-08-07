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
    width: f64,
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

    fn output_character(&mut self, trm: &Transform, width: f64, _spacing: f64, font_size: f64, char: &str) -> Result<(), OutputError> {
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
                width,
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
///
/// Literal space glyphs are handled specially rather than measured like
/// any other character-to-character advance. PDF text draws a real glyph
/// (with its own x-advance, typically similar in width to an ordinary
/// letter) for every space character, so a run of N spaces between two
/// words shows up as N *separate*, individually ordinary-sized deltas —
/// not one single wide gap. That's a trap for any approach that looks at
/// consecutive-glyph deltas alone: a single wide capital letter (e.g. the
/// advance after 'R' or 'W' in Helvetica) can easily be *larger* than one
/// space's own advance, so nothing about delta magnitude alone reliably
/// tells a real column gap apart from an ordinary character advance (this
/// is what previously split "Region" into "R" / "egion" — no fixed or
/// adaptive threshold on individual deltas can fix that, since a single
/// space's delta and a wide letter's delta genuinely overlap). What
/// *does* reliably tell them apart is simply whether the source actually
/// contained one or more literal space characters there at all: we skip
/// space glyphs when appending to the output and instead measure the
/// total gap from the last non-space glyph to the next one, converting
/// that into a proportional number of output spaces (at least one,
/// since we know real whitespace was there). A run of many spaces (a
/// table's column padding) then naturally produces a much wider gap than
/// a single inter-word space, without needing to guess a threshold.
/// A glyph's own `width` (as passed to `output_character`, in text-space
/// units) times its `font_size` predicts its actual on-page advance
/// almost exactly — verified empirically (e.g. a Helvetica capital 'C' at
/// 11pt with `width=0.722` advances by precisely `0.722 * 11 = 7.942`
/// points to the next glyph). That gives a *per-glyph* expected advance
/// to compare against, instead of a single document-wide average: no
/// matter how wide a particular character naturally is (a capital 'W' or
/// 'M'), its own predicted advance already accounts for that, so a
/// same-word transition never looks like a gap just because the glyph
/// happened to be wide.
const JUMP_RATIO: f64 = 1.8;

fn expected_advance(g: &Glyph) -> f64 {
    (g.width * g.font_size).max(0.5)
}

/// Converts each row's glyph positions into a plain string where
/// horizontal spacing is expressed in whole "character cells" derived
/// from the document's own typical glyph pitch, instead of literal PDF
/// point coordinates.
///
/// Two distinct signals feed into deciding where a real word/column gap
/// is, because relying on either alone breaks a real layout:
/// - **Literal space glyphs.** PDF text draws a real glyph for every
///   space character, so a run of N spaces between two words shows up as
///   N *separate*, individually ordinary-sized deltas — not one wide
///   gap. Measuring raw delta magnitude alone can't tell a single space
///   apart from the advance after a wide letter like 'R' or 'W' (both can
///   be similar size), which is what previously split "Region" into "R"
///   / "egion". Skipping space glyphs and folding their width into the
///   gap to the next visible glyph sidesteps that.
/// - **Positioning jumps with no space glyphs at all.** A table built
///   from one separate text-show call per *field* (the common case for
///   real generated PDFs — invoices, reports; see the module docs) has
///   no space characters between columns whatsoever, just a big geometric
///   jump — so literal-space detection alone would glue every field on a
///   row together with nothing between them. [`expected_advance`] catches
///   this: when a glyph's actual on-page advance is far beyond what its
///   own font metrics predict, that gap isn't ordinary character spacing
///   and gets treated as a separator too.
fn rows_to_virtual_text(rows: Vec<Vec<Glyph>>) -> Vec<String> {
    let mut deltas = Vec::new();
    for row in &rows {
        let non_space: Vec<&Glyph> = row.iter().filter(|g| g.ch != ' ').collect();
        for w in non_space.windows(2) {
            let d = w[1].x - w[0].x;
            if d > 0.01 {
                deltas.push(d);
            }
        }
    }
    let unit = median(deltas).unwrap_or(5.0).max(0.5);

    rows.into_iter()
        .map(|row| {
            let mut s = String::new();
            let mut prev_x: Option<f64> = None;
            let mut prev_glyph_advance: Option<f64> = None;
            let mut skipped_spaces = 0u32;
            for g in row {
                if g.ch == ' ' {
                    // Don't emit yet: fold this space's width into the
                    // gap measured against the next non-space glyph, so a
                    // run of several spaces becomes one proportionally
                    // sized gap instead of several individually-normal
                    // deltas.
                    skipped_spaces += 1;
                    continue;
                }

                // Measured from the *end* of the previous visible glyph
                // (its own start position plus its own predicted advance),
                // not from its start — comparing start-to-start double
                // counts the previous glyph's own footprint as part of the
                // gap, which used to turn one real space into two virtual
                // ones (`(y_end_of_word) -> (space) -> (S)` spans two full
                // character pitches start-to-start, but only one pitch of
                // that is actual empty space).
                let gap = match (prev_x, prev_glyph_advance) {
                    (Some(px), Some(adv)) => Some(g.x - (px + adv)),
                    _ => None,
                };
                let is_jump = gap.is_some_and(|gap| gap > unit * (JUMP_RATIO - 1.0));
                if skipped_spaces > 0 || is_jump {
                    if let Some(gap) = gap {
                        let cells = (gap / unit).round().max(1.0) as usize;
                        for _ in 0..cells {
                            s.push(' ');
                        }
                    }
                }
                skipped_spaces = 0;

                s.push(g.ch);
                prev_x = Some(g.x);
                prev_glyph_advance = Some(expected_advance(&g));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one row of glyphs from `(char, width)` pairs at a fixed
    /// font size, positioning each glyph immediately after the previous
    /// one's own predicted advance (`width * font_size`) — i.e. a
    /// perfectly contiguous run with no artificial extra gap, exactly
    /// like real PDF text-space positioning for characters drawn by the
    /// same text-show call.
    fn build_row(chars_and_widths: &[(char, f64)], font_size: f64) -> Vec<Glyph> {
        let mut x = 0.0;
        let mut row = Vec::new();
        for &(ch, width) in chars_and_widths {
            row.push(Glyph {
                x,
                y: 0.0,
                font_size,
                ch,
                width,
            });
            x += width * font_size;
        }
        row
    }

    /// Real measured Helvetica-11pt widths (text-space units, i.e. what
    /// `pdf-extract`'s `output_character` passes as `width`) for the
    /// characters used below — captured empirically while diagnosing the
    /// word-splitting bug this test guards against.
    const R: (char, f64) = ('R', 0.722);
    const E: (char, f64) = ('e', 0.556);
    const G: (char, f64) = ('g', 0.556);
    const I: (char, f64) = ('i', 0.222);
    const O: (char, f64) = ('o', 0.556);
    const N: (char, f64) = ('n', 0.556);
    const SPACE: (char, f64) = (' ', 0.278);
    const U: (char, f64) = ('U', 0.667);
    const T: (char, f64) = ('t', 0.278);
    const S: (char, f64) = ('s', 0.5);

    #[test]
    fn a_wide_capital_letters_own_advance_does_not_get_mistaken_for_a_gap() {
        // "Region" — the wide 'R' at the start was exactly the case that
        // used to get split into "R" / "egion": its own on-page advance
        // (0.722 * 11 = 7.942) is bigger than a real space's advance
        // (0.278 * 11 = 3.058), so magnitude-only heuristics saw it as
        // "bigger than a space, must be a gap."
        let row = build_row(&[R, E, G, I, O, N], 11.0);
        let text = rows_to_virtual_text(vec![row]);
        assert_eq!(text, vec!["Region".to_string()]);
    }

    #[test]
    fn a_real_single_space_produces_exactly_one_virtual_space() {
        let row = build_row(&[R, E, G, I, O, N, SPACE, U, N, I, T, S], 11.0);
        let text = rows_to_virtual_text(vec![row]);
        assert_eq!(text, vec!["Region Units".to_string()]);
    }

    #[test]
    fn a_run_of_several_spaces_produces_a_proportionally_wider_gap() {
        let mut chars = vec![R, E, G, I, O, N];
        chars.extend([SPACE; 6]);
        chars.extend([U, N, I, T, S]);
        let row = build_row(&chars, 11.0);
        let text = rows_to_virtual_text(vec![row]);
        let rendered = &text[0];

        assert!(rendered.starts_with("Region"));
        assert!(rendered.ends_with("Units"));
        let gap_len = rendered.len() - "Region".len() - "Units".len();
        assert!(
            gap_len > 1,
            "a 6-space run should render wider than a single inter-word space, got {rendered:?}"
        );
    }

    #[test]
    fn a_positioning_jump_with_no_space_glyphs_still_separates_fields() {
        // The "one text-show call per field" case (real invoice/report
        // generators): no space glyph at all, just a big geometric jump.
        let mut row = build_row(&[R, E, G, I, O, N], 11.0);
        let jump_start_x = row.last().unwrap().x + expected_advance(row.last().unwrap()) + 50.0;
        let mut second_field = build_row(&[U, N, I, T, S], 11.0);
        for g in &mut second_field {
            g.x += jump_start_x;
        }
        row.extend(second_field);

        let text = rows_to_virtual_text(vec![row]);
        let rendered = &text[0];
        assert!(rendered.starts_with("Region"));
        assert!(rendered.ends_with("Units"));
        let gap_len = rendered.len() - "Region".len() - "Units".len();
        assert!(
            gap_len > 3,
            "a 50pt positioning jump with no space glyphs should still render a wide gap, got {rendered:?}"
        );
    }
}
