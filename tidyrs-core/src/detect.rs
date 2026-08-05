use crate::parser::TidyParser;

/// Registry of all known parsers. This is the "format detection engine
/// upstream" from the spec: it identifies the real type of a file beyond
/// its extension by asking every registered parser how confident it is,
/// then picking the best match.
#[derive(Default)]
pub struct FormatRegistry {
    parsers: Vec<Box<dyn TidyParser>>,
}

pub struct Detection<'a> {
    pub parser: &'a dyn TidyParser,
    pub confidence: f32,
}

impl FormatRegistry {
    pub fn new() -> Self {
        Self { parsers: Vec::new() }
    }

    pub fn register(&mut self, parser: Box<dyn TidyParser>) -> &mut Self {
        self.parsers.push(parser);
        self
    }

    /// Returns the parser with the highest sniff confidence, if any parser
    /// is confident enough (> 0.0) to be considered.
    pub fn detect(&self, bytes: &[u8], filename: Option<&str>) -> Option<Detection<'_>> {
        self.parsers
            .iter()
            .map(|p| Detection {
                parser: p.as_ref(),
                confidence: p.sniff(bytes, filename),
            })
            .filter(|d| d.confidence > 0.0)
            .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
    }

    pub fn by_name(&self, name: &str) -> Option<&dyn TidyParser> {
        self.parsers.iter().find(|p| p.format_name() == name).map(|p| p.as_ref())
    }
}
