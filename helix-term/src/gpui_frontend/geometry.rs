//! Helix visual rows and source-character to displayed-byte mapping.
use std::ops::Range;

#[cfg(test)]
use helix_core::unicode::width::UnicodeWidthStr;

// helix-gpui's document TextStyle uses 14px with GPUI's default golden-ratio line height.
pub(super) const FONT_FAMILY: &str = "JetBrains Mono";
pub(super) const FONT_SIZE: f32 = 14.;
pub(super) const LINE_HEIGHT: f32 = FONT_SIZE * 1.618_034;
pub(super) const INSET_X: f32 = 2.;
pub(super) const INSET_Y: f32 = 1.;
pub(super) struct DisplayLine {
    pub text: String,
    spans: Vec<(Range<usize>, Range<usize>)>,
}

impl DisplayLine {
    #[cfg(test)]
    fn new(source: &str, tab_width: usize) -> Self {
        let text = helix_core::Rope::from_str(source);
        let format = helix_core::doc_formatter::TextFormat {
            tab_width: tab_width as u16,
            ..Default::default()
        };
        visual_rows(text.slice(..), &format, &Default::default())
            .remove(0)
            .display
    }

    pub fn display_range(&self, source: Range<usize>) -> Option<Range<usize>> {
        if source.is_empty() {
            return None;
        }
        let mut matching = self
            .spans
            .iter()
            .filter(|(chars, _)| source.start < chars.end && chars.start < source.end);
        let (_, first) = matching.next()?;
        let end = matching.last().map_or(first.end, |(_, bytes)| bytes.end);
        Some(first.start..end)
    }
}

/// One screen row. Source spans use absolute document character positions.
pub(super) struct VisualRow {
    pub line: usize,
    pub first: bool,
    pub source: Range<usize>,
    pub display: DisplayLine,
}

pub(super) fn visual_rows(
    text: helix_core::RopeSlice<'_>,
    format: &helix_core::doc_formatter::TextFormat,
    annotations: &helix_core::text_annotations::TextAnnotations<'_>,
) -> Vec<VisualRow> {
    use helix_core::{doc_formatter::DocumentFormatter, graphemes::Grapheme};
    let mut rows: Vec<VisualRow> = Vec::new();
    let mut column = 0;
    for grapheme in DocumentFormatter::new_at_prev_checkpoint(text, format, annotations, 0) {
        if rows.len() <= grapheme.visual_pos.row {
            let first = rows.last().is_none_or(|row| row.line != grapheme.line_idx);
            while rows.len() <= grapheme.visual_pos.row {
                rows.push(VisualRow {
                    line: grapheme.line_idx,
                    first,
                    source: grapheme.char_idx..grapheme.char_idx,
                    display: DisplayLine {
                        text: String::new(),
                        spans: Vec::new(),
                    },
                });
            }
            column = 0;
        }
        let row = rows.last_mut().unwrap();
        row.display.text.extend(std::iter::repeat_n(
            ' ',
            grapheme.visual_pos.col.saturating_sub(column),
        ));
        let start = row.display.text.len();
        match &grapheme.raw {
            Grapheme::Other { g } => row.display.text.push_str(g),
            _ => row
                .display
                .text
                .extend(std::iter::repeat_n(' ', grapheme.width())),
        }
        if !grapheme.is_virtual() {
            let end = grapheme.char_idx + grapheme.doc_chars().max(1);
            row.display
                .spans
                .push((grapheme.char_idx..end, start..row.display.text.len()));
            row.source.end = end;
        }
        column = grapheme.visual_pos.col + grapheme.width();
    }
    rows
}

pub(super) fn row_at_char(rows: &[VisualRow], character: usize) -> usize {
    rows.iter()
        .position(|row| row.source.contains(&character))
        .unwrap_or_else(|| rows.len().saturating_sub(1))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_rows_match_helix_positions() {
        use helix_core::{
            doc_formatter::TextFormat, text_annotations::TextAnnotations, visual_offset_from_block,
            Rope,
        };
        let text =
            Rope::from_str("    alpha beta 界 e\u{301} 👩‍💻 gamma delta\tend\r\nsecond line\n");
        let annotations = TextAnnotations::default();
        for width in [12, 20, 80] {
            let format = TextFormat {
                soft_wrap: true,
                viewport_width: width,
                wrap_indicator: "".into(),
                ..TextFormat::default()
            };
            let rows = visual_rows(text.slice(..), &format, &annotations);
            assert_eq!(
                rows.iter().filter(|row| row.first).count(),
                text.len_lines()
            );
            for character in 0..=text.len_chars() {
                let expected =
                    visual_offset_from_block(text.slice(..), 0, character, &format, &annotations).0;
                let row = row_at_char(&rows, character);
                assert_eq!(row, expected.row, "width={width}, char={character}");
                let bytes = rows[row]
                    .display
                    .display_range(character..character + 1)
                    .unwrap();
                assert_eq!(rows[row].display.text[..bytes.start].width(), expected.col);
            }
            assert!(rows
                .iter()
                .all(|row| row.display.text.width() <= width as usize));
            if width == 12 {
                assert!(rows.len() > text.len_lines());
                assert!(!rows[1].first);
            }
        }
    }

    #[test]
    fn disabled_wrapping_preserves_long_rows() {
        use helix_core::{doc_formatter::TextFormat, text_annotations::TextAnnotations, Rope};
        let text = Rope::from_str("a long line that exceeds the viewport\nnext");
        let rows = visual_rows(
            text.slice(..),
            &TextFormat {
                viewport_width: 12,
                ..TextFormat::default()
            },
            &TextAnnotations::default(),
        );
        assert_eq!(rows.len(), 2);
        assert!(rows[0].display.text.width() > 12);
        assert!(rows.iter().all(|row| row.first));
    }
    #[test]
    fn tabs_follow_visual_columns() {
        let line = DisplayLine::new("界\tx\t", 4);
        assert_eq!(line.text, "界  x    ");
        assert_eq!(line.display_range(1..2), Some(3..5));
        assert_eq!(line.display_range(3..4), Some(6..9));
    }

    #[test]
    fn cursor_covers_combining_and_joined_graphemes() {
        let line = DisplayLine::new("e\u{301}👩‍💻z", 4);
        assert_eq!(line.display_range(0..1), Some(0..3));
        assert_eq!(line.display_range(1..2), Some(0..3));
        assert_eq!(line.display_range(3..4), Some(3..14));
        assert_eq!(line.display_range(2..6), Some(3..15));
        assert_eq!(line.display_range(1..1), None);
    }

    #[test]
    fn line_end_has_a_cursor_cell() {
        for source in ["λ", "λ\n", "λ\r\n"] {
            let line = DisplayLine::new(source, 4);
            assert_eq!(line.text, "λ ");
            assert_eq!(line.display_range(1..2), Some(2..3));
            assert_eq!(line.display_range(0..0), None);
            assert_eq!(line.display_range(10..11), None);
        }
    }
}
