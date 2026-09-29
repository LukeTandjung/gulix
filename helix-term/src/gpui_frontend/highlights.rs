//! Visible source styles, using the current Helix highlighter and overlay helpers.
//!
//! Like `helix-gpui`'s document style iterator, resolve syntax first, then patch
//! overlay styles onto it. Tree-house supplies boundary events instead of the
//! old Tree-sitter `HighlightStart` / `Source` event stream.

use std::ops::Range;

use helix_core::{
    syntax::{HighlightEvent, Highlighter, OverlayHighlighter, OverlayHighlights},
    RopeSlice,
};
use helix_stdx::rope::RopeSliceExt;
use helix_view::{graphics::Style, Document, Editor, Theme, View};

use crate::ui::EditorView;

/// Return ordered, disjoint, merged styles in absolute source character indices.
///
/// `visible` may include `doc.text().len_chars()..len_chars() + 1` for the
/// synthetic EOF cursor cell. Syntax queries cover only the requested source
/// bytes. Overlay helpers inspect diagnostic/selection metadata, not all source
/// text, and their results are clipped before event iteration.
///
/// Pass editor-view focus and native window focus separately. Helix omits the
/// primary block cursor when the window is unfocused. Bar/underline cursors and
/// the unfocused cursor outline need separate GPUI painting.
pub(super) fn visible_styles(
    editor: &Editor,
    doc: &Document,
    view: &View,
    visible: Range<usize>,
    is_view_focused: bool,
    is_window_focused: bool,
) -> Vec<(Range<usize>, Style)> {
    let text = doc.text().slice(..);
    let visible = visible.start.min(text.len_chars() + 1)..visible.end.min(text.len_chars() + 1);
    if visible.is_empty() {
        return Vec::new();
    }

    let theme = &editor.theme;
    let config = editor.config();
    let loader = editor.syn_loader.load();
    let source_start = visible.start.min(text.len_chars());
    let source_end = visible.end.min(text.len_chars());
    let syntax = doc
        .syntax()
        .filter(|_| source_start < source_end)
        .map(|syntax| {
            syntax.highlighter(
                text,
                &loader,
                text.char_to_byte(source_start) as u32..text.char_to_byte(source_end) as u32,
            )
        });

    let first_line = text.char_to_line(source_start);
    let last_line = text.char_to_line(source_end.saturating_sub(1).max(source_start));
    let height = (last_line - first_line + 1).min(u16::MAX as usize) as u16;
    let annotations = view.text_annotations(doc, Some(theme));
    let mut overlays = vec![annotations.collect_overlay_highlights(visible.clone())];
    if doc
        .language_config()
        .and_then(|language| language.rainbow_brackets)
        .unwrap_or(config.rainbow_brackets)
    {
        overlays.extend(EditorView::doc_rainbow_highlights(
            doc,
            source_start,
            height,
            theme,
            &loader,
        ));
    }
    overlays.extend(EditorView::doc_document_link_highlights(doc, theme));
    EditorView::doc_diagnostics_highlights_into(doc, theme, &mut overlays);
    if is_view_focused {
        if config.lsp.auto_document_highlight {
            overlays.extend(EditorView::doc_document_highlights(doc, view, theme));
        }
        overlays.extend(EditorView::tabstop_highlights(doc, theme));
        overlays.extend(EditorView::highlight_focused_view_elements(
            view, doc, theme,
        ));
        // Selection/cursor styles have final precedence, including at a matched brace.
        overlays.push(EditorView::doc_selection_highlights(
            editor.mode(),
            doc,
            view,
            theme,
            &config.cursor_shape,
            is_window_focused,
        ));
    }
    resolve_runs(text, syntax, overlays, theme, visible)
}

fn clip_overlay(overlay: OverlayHighlights, visible: &Range<usize>) -> OverlayHighlights {
    let clip = |range: &mut Range<usize>| {
        range.start = range.start.max(visible.start);
        range.end = range.end.min(visible.end);
        range.start < range.end
    };
    match overlay {
        OverlayHighlights::Homogeneous {
            highlight,
            mut ranges,
        } => {
            ranges.retain_mut(clip);
            OverlayHighlights::Homogeneous { highlight, ranges }
        }
        OverlayHighlights::Heterogenous { mut highlights } => {
            highlights.retain_mut(|(_, range)| clip(range));
            OverlayHighlights::Heterogenous { highlights }
        }
    }
}

fn next_syntax_offset(syntax: &Option<Highlighter<'_>>, text: RopeSlice<'_>) -> usize {
    syntax.as_ref().map_or(usize::MAX, |syntax| {
        let byte = syntax.next_event_offset();
        if byte == u32::MAX {
            usize::MAX
        } else {
            // Use the same character-boundary rounding as the terminal renderer.
            text.byte_to_char(text.ceil_char_boundary(byte as usize))
        }
    })
}

fn resolve_runs(
    text: RopeSlice<'_>,
    mut syntax: Option<Highlighter<'_>>,
    overlays: Vec<OverlayHighlights>,
    theme: &Theme,
    visible: Range<usize>,
) -> Vec<(Range<usize>, Style)> {
    let mut overlay = OverlayHighlighter::new(
        overlays
            .into_iter()
            .map(|overlay| clip_overlay(overlay, &visible)),
    );
    let base = theme.get("ui.text");
    let mut syntax_style = base;
    let mut overlay_style = Style::default();
    let mut runs = Vec::new();
    let mut pos = visible.start;
    while pos < visible.end {
        if pos >= text.len_chars() {
            // The synthetic EOF cell is not part of the syntax tree.
            syntax = None;
            syntax_style = base;
        }
        while next_syntax_offset(&syntax, text) <= pos {
            let (event, highlights) = syntax.as_mut().unwrap().advance();
            if event == HighlightEvent::Refresh {
                syntax_style = base;
            }
            syntax_style = highlights.fold(syntax_style, |style, highlight| {
                style.patch(theme.highlight(highlight))
            });
        }
        while overlay.next_event_offset() <= pos {
            let (event, highlights) = overlay.advance();
            if event == HighlightEvent::Refresh {
                overlay_style = Style::default();
            }
            overlay_style = highlights.fold(overlay_style, |style, highlight| {
                style.patch(theme.highlight(highlight))
            });
        }
        let mut end = visible
            .end
            .min(next_syntax_offset(&syntax, text))
            .min(overlay.next_event_offset());
        if pos < text.len_chars() {
            end = end.min(text.len_chars());
        }
        push_run(&mut runs, pos..end, syntax_style.patch(overlay_style));
        pos = end;
    }
    runs
}

fn push_run(runs: &mut Vec<(Range<usize>, Style)>, range: Range<usize>, style: Style) {
    if range.is_empty() {
        return;
    }
    if let Some((previous, previous_style)) = runs.last_mut() {
        if previous.end == range.start && *previous_style == style {
            previous.end = range.end;
            return;
        }
    }
    runs.push((range, style));
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_core::{syntax::Highlight, Rope};
    use helix_view::{graphics::Color, theme::DEFAULT_THEME};

    #[test]
    fn merges_only_adjacent_equal_styles() {
        let style = Style::default().fg(Color::Red);
        let mut runs = Vec::new();
        push_run(&mut runs, 3..5, style);
        push_run(&mut runs, 5..8, style);
        push_run(&mut runs, 9..10, style);
        push_run(&mut runs, 10..10, style);
        assert_eq!(runs, vec![(3..8, style), (9..10, style)]);
    }

    #[test]
    fn clips_overlays_and_patches_them_in_priority_order() {
        let text = Rope::from_str("aλbcdefgh");
        let theme = &DEFAULT_THEME;
        let diagnostic = theme.find_highlight_exact("diagnostic.error").unwrap();
        let selection = theme.find_highlight_exact("ui.selection").unwrap();
        let layers = vec![
            OverlayHighlights::single(diagnostic, 0..8),
            OverlayHighlights::single(selection, 4..6),
        ];
        let runs = resolve_runs(text.slice(..), None, layers, theme, 2..7);
        let diagnostic_style = theme.get("ui.text").patch(theme.highlight(diagnostic));
        assert_eq!(
            runs,
            vec![
                (2..4, diagnostic_style),
                (4..6, diagnostic_style.patch(theme.highlight(selection))),
                (6..7, diagnostic_style),
            ]
        );
    }

    #[test]
    fn includes_eof_cursor_cell_without_scanning_source_prefix() {
        let text = Rope::from_str("λ\n");
        let theme = &DEFAULT_THEME;
        let cursor: Highlight = theme.find_highlight_exact("ui.cursor").unwrap();
        let runs = resolve_runs(
            text.slice(..),
            None,
            vec![OverlayHighlights::single(cursor, 2..3)],
            theme,
            2..3,
        );
        assert_eq!(
            runs,
            vec![(2..3, theme.get("ui.text").patch(theme.highlight(cursor)))]
        );
    }
}
