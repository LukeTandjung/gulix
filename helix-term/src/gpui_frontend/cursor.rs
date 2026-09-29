//! Primary cursor geometry adapted from helix-gpui's document.rs.
use gpui::{div, prelude::*, px, Div};
use helix_core::unicode::width::UnicodeWidthStr;
use helix_view::{graphics::CursorKind, Document, Editor, View};

use super::{
    geometry::{DisplayLine, LINE_HEIGHT},
    styling,
};

pub(super) fn render(
    editor: &Editor,
    doc: &Document,
    view: &View,
    line: &DisplayLine,
    start: usize,
    horizontal_offset: usize,
    cell: f32,
) -> Option<Div> {
    let position = doc
        .selection(view.id)
        .primary()
        .cursor(doc.text().slice(..));
    if position < start {
        return None;
    }
    let bytes = line.display_range(position - start..position - start + 1)?;
    let column = line.text[..bytes.start].width();
    if column < horizontal_offset {
        return None;
    }
    let kind = editor.config().cursor_shape.from_mode(editor.mode());
    let (width, height, top) = match kind {
        CursorKind::Bar => (2., LINE_HEIGHT, 0.),
        CursorKind::Block => (cell, LINE_HEIGHT, 0.),
        CursorKind::Underline => (cell, 2., LINE_HEIGHT - 2.),
        CursorKind::Hidden => return None,
    };
    let base = styling::highlight(styling::base_style(editor));
    let mut color = editor
        .theme
        .get("ui.cursor.primary")
        .bg
        .and_then(styling::color_to_hsla)
        .or(base.color)
        .unwrap_or_else(gpui::white);
    color.a = 0.5;
    Some(
        div()
            .absolute()
            .left(px((column - horizontal_offset) as f32 * cell))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .bg(color),
    )
}
