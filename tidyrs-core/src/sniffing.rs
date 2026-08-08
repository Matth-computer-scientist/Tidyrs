//! Shared helper for text-based format sniffing (`tidyrs-csv`,
//! `tidyrs-fixed`): a bounded but representative sample of a file's
//! bytes, used instead of reading the whole file just to guess its
//! format.
//!
//! A single leading prefix is *not* representative enough on its own: a
//! real export can plausibly have several KB of preamble (a comment
//! block, a metadata section, a wide free-text column) before the actual
//! tabular content starts, and a sniffer that only ever looks at the
//! first few KB would misdetect the file entirely, having never seen the
//! part that actually looks like a table. Sampling the start, middle, and
//! end instead means the real content is very likely represented
//! somewhere in what gets scored, at a bounded, predictable cost
//! regardless of file size.

/// Total sniffing sample size. Kept modest — this runs once per file per
/// candidate format during detection, not on the hot parsing path.
const SAMPLE_LIMIT: usize = 12_288;

/// Strips a leading UTF-8 byte-order mark (`EF BB BF`) if present. A BOM
/// is valid UTF-8 (it decodes to U+FEFF) so `str::from_utf8`/
/// `from_utf8_lossy` never reject it — left in place, it silently glues
/// itself onto whatever the first character of the file "means" to a
/// parser (a CSV/INI header's first name, a fixed-width file's first
/// column), which is a real correctness bug found via external QA
/// testing, not just a display quirk. Every text-based parser here should
/// call this before decoding, the same way binary formats check a magic
/// header before trusting their own byte offsets.
pub fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

/// Returns up to [`SAMPLE_LIMIT`] bytes representative of `bytes`: the
/// whole input if it's already small, otherwise a prefix, a middle
/// chunk, and a suffix chunk concatenated together. Chunk boundaries can
/// land mid-line (or, for multi-byte encodings, mid-character) — that's
/// fine for sniffing, which only needs *most* of the sampled lines to be
/// clean, not every single one; the real parse afterward always reads
/// the whole file properly regardless of what was sampled here.
pub fn sample_for_sniffing(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() <= SAMPLE_LIMIT {
        return bytes.to_vec();
    }

    let chunk = SAMPLE_LIMIT / 3;
    let mid_start = (bytes.len() / 2).saturating_sub(chunk / 2);

    let mut sample = Vec::with_capacity(SAMPLE_LIMIT);
    sample.extend_from_slice(&bytes[..chunk]);
    sample.push(b'\n');
    sample.extend_from_slice(&bytes[mid_start..(mid_start + chunk).min(bytes.len())]);
    sample.push(b'\n');
    sample.extend_from_slice(&bytes[bytes.len() - chunk..]);
    sample
}

/// Picks up to `n` non-empty lines from `text`, split between the start
/// and the end rather than just the first `n` — the same "don't only
/// look at the head" reasoning as [`sample_for_sniffing`], one level
/// down: even within an already-bounded sample, code that then does
/// `.lines().take(n)` is right back to only ever seeing the very
/// beginning of whatever it was given.
pub fn representative_lines(text: &str, n: usize) -> Vec<&str> {
    let non_empty: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if non_empty.len() <= n {
        return non_empty;
    }
    let head = n.div_ceil(2);
    let tail = n - head;
    let mut lines: Vec<&str> = non_empty[..head].to_vec();
    lines.extend_from_slice(&non_empty[non_empty.len() - tail..]);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_input_is_returned_unchanged() {
        let bytes = b"a,b,c\n1,2,3\n".to_vec();
        assert_eq!(sample_for_sniffing(&bytes), bytes);
    }

    #[test]
    fn large_input_includes_content_from_the_end() {
        let mut bytes = vec![b'a'; 20_000];
        bytes.extend_from_slice(b"UNIQUE_TAIL_MARKER");
        let sample = sample_for_sniffing(&bytes);
        assert!(sample.len() <= SAMPLE_LIMIT + 2);
        assert!(sample.windows(18).any(|w| w == b"UNIQUE_TAIL_MARKER"));
    }

    #[test]
    fn large_input_includes_content_from_the_middle() {
        let mut bytes = vec![b'a'; 10_000];
        bytes.extend_from_slice(b"UNIQUE_MIDDLE_MARKER");
        bytes.extend(vec![b'b'; 10_000]);
        let sample = sample_for_sniffing(&bytes);
        assert!(sample.windows(20).any(|w| w == b"UNIQUE_MIDDLE_MARKER"));
    }

    #[test]
    fn representative_lines_includes_lines_from_the_end_not_just_the_start() {
        let text: String = (0..50).map(|i| format!("line{i}\n")).collect();
        let lines = representative_lines(&text, 10);
        assert_eq!(lines.len(), 10);
        assert!(lines.contains(&"line0"));
        assert!(lines.contains(&"line49"), "expected a line from the end, got {lines:?}");
    }

    #[test]
    fn representative_lines_returns_everything_when_under_the_limit() {
        let lines = representative_lines("a\nb\nc\n", 10);
        assert_eq!(lines, vec!["a", "b", "c"]);
    }
}
