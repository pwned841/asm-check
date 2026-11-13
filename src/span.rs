//! Source locations.

/// A region of a single source line.
///
/// Lines are 0-based. `start` and `end` are byte offsets into the line, with
/// `end` exclusive. Front ends convert them to whatever their protocol needs
/// (1-based columns for the terminal, UTF-16 code units for LSP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(line: usize, start: usize, end: usize) -> Self {
        Self { line, start, end }
    }
}
