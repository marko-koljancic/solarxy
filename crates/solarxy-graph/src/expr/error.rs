//! The one error type the expression language produces.

use std::ops::Range;

/// A parse or evaluation failure, carrying the byte span of the offending
/// source so the editor can underline it.
///
/// Derives `Eq` deliberately: it is carried inside
/// [`crate::registry::resolve::ResolveFailure`], which is `Eq`, and a
/// non-comparable payload there would break every test that compares a
/// resolve outcome.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ExprError {
    pub message: String,
    /// Byte offsets into the source, suitable for slicing.
    pub span: Range<usize>,
}

impl ExprError {
    pub fn new(message: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    /// The 1-based line and column the span starts at, for editors that
    /// speak in coordinates rather than offsets.
    ///
    /// Counts characters, not bytes, so a column lands where a human sees
    /// it in a line containing multi-byte text.
    #[must_use]
    pub fn line_col(&self, source: &str) -> (usize, usize) {
        let upto = &source[..self.span.start.min(source.len())];
        let line = upto.bytes().filter(|b| *b == b'\n').count() + 1;
        let col = upto
            .rsplit_once('\n')
            .map_or(upto, |(_, tail)| tail)
            .chars()
            .count()
            + 1;
        (line, col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_is_one_based_on_the_first_line() {
        let e = ExprError::new("x", 4..5);
        assert_eq!(e.line_col("1 + $Q"), (1, 5));
    }

    #[test]
    fn line_col_counts_lines_and_restarts_the_column() {
        let src = "a;\nbb;\nccc";
        // Offset 7 is the first 'c' on line 3.
        let e = ExprError::new("x", 7..8);
        assert_eq!(e.line_col(src), (3, 1));
    }

    #[test]
    fn line_col_counts_characters_not_bytes() {
        // Three 2-byte chars then the offending token at byte 6.
        let src = "ααα!";
        let e = ExprError::new("x", 6..7);
        assert_eq!(e.line_col(src), (1, 4));
    }

    #[test]
    fn an_out_of_range_span_does_not_panic() {
        let e = ExprError::new("x", 99..100);
        assert_eq!(e.line_col("short"), (1, 6));
    }
}
