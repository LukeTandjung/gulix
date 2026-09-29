//! Native presentation of the authoritative Helix prompt.
//! Layout follows helix-gpui's prompt.rs and overlay.rs at 79ec318c.
//! Unlike that reference, this reads prompt data, not a terminal buffer.
use gpui::{div, prelude::*, px, Div, StyledText, Window};
use helix_core::unicode::{segmentation::UnicodeSegmentation, width::UnicodeWidthStr};
use helix_view::{graphics::Style, Editor};

use super::{base_style, highlight};
use crate::ui::Prompt;

const FONT_SIZE: f32 = 12.;
const LINE_HEIGHT: f32 = 15.6;

fn text(text: String, style: Style) -> StyledText {
    let len = text.len();
    StyledText::new(text).with_highlights([(0..len, highlight(style))])
}

/// Keep the caret visible without adding a second editable prompt state.
fn visible_input(line: &str, cursor: usize, columns: usize) -> (String, usize) {
    let mut start = 0;
    let mut width = line[..cursor].width();
    for (index, grapheme) in line[..cursor].grapheme_indices(true) {
        if width < columns {
            break;
        }
        width = width.saturating_sub(grapheme.width());
        start = index + grapheme.len();
    }
    let mut result = String::new();
    let mut used = 0;
    for grapheme in line[start..].graphemes(true) {
        if used + grapheme.width() > columns {
            break;
        }
        result.push_str(grapheme);
        used += grapheme.width();
    }
    (result, width)
}

/// Returns a centered overlay. Add it last to a positioned workspace root.
/// Keyboard events still route to the existing Helix Prompt through Workspace.
pub(in crate::gpui_frontend) fn render(editor: &Editor, prompt: &Prompt, window: &Window) -> Div {
    let font_id = window
        .text_system()
        .resolve_font(&gpui::font("JetBrains Mono"));
    let cell = window
        .text_system()
        .advance(font_id, px(FONT_SIZE), 'm')
        .map(|size| f32::from(size.width))
        .unwrap_or(7.2)
        .max(1.);
    // The reference paints two thirds of the Helix area at its smaller prompt font.
    let columns = (editor.tree.area().width as usize * 2 / 3)
        .min(((f32::from(window.viewport_size().width) - 40.).max(cell) / cell) as usize)
        .max(1);
    let base = base_style(editor);
    let mut panel = div()
        .flex()
        .flex_col()
        .p(px(20.))
        .rounded_sm()
        .shadow_sm()
        .font_family("JetBrains Mono")
        .text_size(px(FONT_SIZE))
        .line_height(px(LINE_HEIGHT))
        .bg(highlight(base).background_color.unwrap())
        .text_color(highlight(base).color.unwrap());
    let mut content = div()
        .flex()
        .flex_col()
        .w(px(columns as f32 * cell))
        .overflow_hidden();

    if let Some(documentation) = (prompt.doc_fn)(prompt.line()) {
        let help = base.patch(editor.theme.get("ui.help"));
        content = content.child(
            div()
                .border_1()
                .border_color(highlight(help).color.unwrap())
                .px(px(cell))
                .max_h(px(LINE_HEIGHT * 8.))
                .overflow_hidden()
                .bg(highlight(help).background_color.unwrap())
                .child(text(documentation.into_owned(), help)),
        );
    }

    let completions = prompt.completions();
    let longest = completions
        .iter()
        .map(|(_, item)| item.content.width())
        .max()
        .unwrap_or(30)
        .max(30);
    let cols = (columns / longest).max(1);
    let rows = completions.len().div_ceil(cols).min(10);
    if rows > 0 {
        let page_size = rows * cols;
        let offset = prompt.completion_selection().unwrap_or(0) / page_size * page_size;
        let menu_style = base.patch(editor.theme.get("ui.menu"));
        let col_width = columns.saturating_sub(cols) / cols;
        let mut menu = div()
            .flex()
            .bg(highlight(menu_style).background_color.unwrap());
        for col in 0..cols {
            let mut column = div()
                .flex()
                .flex_col()
                .w(px((col_width + 1) as f32 * cell))
                .flex_shrink_0();
            for row in 0..rows {
                let index = offset + col * rows + row;
                let mut item = div().h(px(LINE_HEIGHT)).overflow_hidden();
                if let Some((_, completion)) = completions.get(index) {
                    let style = if prompt.completion_selection() == Some(index) {
                        base.patch(editor.theme.get("ui.menu.selected"))
                    } else {
                        menu_style.patch(completion.style)
                    };
                    item = item.child(text(completion.content.to_string(), style));
                }
                column = column.child(item);
            }
            menu = menu.child(column);
        }
        content = content.child(menu);
    }

    let prefix_width = prompt.prefix().width();
    let input_columns = columns.saturating_sub(prefix_width).max(1);
    let (visible, cursor_column) = visible_input(prompt.line(), prompt.position(), input_columns);
    let mut input = div()
        .relative()
        .h(px(LINE_HEIGHT))
        .overflow_hidden()
        .flex()
        .child(text(prompt.prefix().to_owned(), base));
    if prompt.line().is_empty() {
        if let Some(suggestion) = prompt.first_history_completion(editor) {
            input = input.child(text(
                suggestion.into_owned(),
                base.patch(editor.theme.get("ui.text.inactive")),
            ));
        }
    } else {
        input = input.child(text(visible, base));
    }
    let cursor = highlight(base.patch(editor.theme.get("ui.cursor.primary")));
    input = input.child(
        div()
            .absolute()
            .left(px((prefix_width + cursor_column) as f32 * cell))
            .top_0()
            .w(px(1.))
            .h(px(LINE_HEIGHT))
            .bg(cursor.background_color.or(cursor.color).unwrap()),
    );
    content = content.child(input);
    panel = panel.child(content);
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(panel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_window_keeps_unicode_caret_visible() {
        assert_eq!(
            visible_input("a界e\u{301}xyz", "a界e\u{301}".len(), 4),
            ("界e\u{301}x".into(), 3)
        );
        assert_eq!(visible_input("abcdef", 6, 4), ("def".into(), 3));
        assert_eq!(visible_input("abcdef", 0, 4), ("abcd".into(), 0));
    }
}
