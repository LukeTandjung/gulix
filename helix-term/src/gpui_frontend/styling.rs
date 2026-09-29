//! Adapted from helix-gpui's utils.rs, document.rs (Gutter), and statusline.rs
//! at 79ec318ca9f020dca7456c2d5873994836928fe8. Use current Helix styles and
//! span producers, with GPUI painting rather than a terminal surface.
use gpui::{div, prelude::*, px, rgb, Div, HighlightStyle, Hsla, StyledText};
use helix_view::{
    graphics::{Color, Modifier, Style},
    Document, Editor, View,
};

use crate::ui;

#[path = "prompt.rs"]
pub(super) mod prompt;

pub(super) fn color_to_hsla(color: Color) -> Option<Hsla> {
    let index = match color {
        Color::Reset => return None,
        Color::Rgb(r, g, b) => {
            return Some(rgb((r as u32) << 16 | (g as u32) << 8 | b as u32).into())
        }
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::LightGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(index) => index,
    };
    let value = match index {
        0..=15 => [
            0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xc0c0c0,
            0x808080, 0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
        ][index as usize],
        16..=231 => {
            let n = (index - 16) as usize;
            let levels = [0, 95, 135, 175, 215, 255];
            levels[n / 36] << 16 | levels[n / 6 % 6] << 8 | levels[n % 6]
        }
        _ => {
            let gray = 8 + (index as u32 - 232) * 10;
            gray << 16 | gray << 8 | gray
        }
    };
    Some(rgb(value).into())
}

pub(super) fn base_style(editor: &Editor) -> Style {
    Style::default()
        .fg(Color::White)
        .bg(Color::Black)
        .patch(editor.theme.get("ui.background"))
        .patch(editor.theme.get("ui.text"))
}

pub(super) fn highlight(style: Style) -> HighlightStyle {
    let mut fg = style.fg.and_then(color_to_hsla);
    let mut bg = style.bg.and_then(color_to_hsla);
    if style.add_modifier.contains(Modifier::REVERSED) {
        std::mem::swap(&mut fg, &mut bg);
    }
    HighlightStyle {
        color: fg,
        background_color: bg,
        font_weight: style
            .add_modifier
            .contains(Modifier::BOLD)
            .then_some(gpui::FontWeight::BOLD),
        font_style: style
            .add_modifier
            .contains(Modifier::ITALIC)
            .then_some(gpui::FontStyle::Italic),
        underline: style
            .underline_style
            .filter(|kind| *kind != helix_view::graphics::UnderlineStyle::Reset)
            .map(|kind| gpui::UnderlineStyle {
                thickness: px(1.),
                color: style.underline_color.and_then(color_to_hsla).or(fg),
                wavy: kind == helix_view::graphics::UnderlineStyle::Curl,
            }),
        strikethrough: style
            .add_modifier
            .contains(Modifier::CROSSED_OUT)
            .then_some(gpui::StrikethroughStyle {
                thickness: px(1.),
                color: fg,
            }),
        fade_out: style.add_modifier.contains(Modifier::DIM).then_some(0.5),
        ..Default::default()
    }
}

fn styled_spans(spans: tui::text::Spans<'_>) -> StyledText {
    let mut text = String::new();
    let mut runs = Vec::new();
    for span in spans.0 {
        let start = text.len();
        text.push_str(&span.content);
        if start < text.len() {
            runs.push((start..text.len(), highlight(span.style)));
        }
    }
    StyledText::new(text).with_highlights(runs)
}

pub(super) fn gutter(editor: &Editor, doc: &Document, view: &View, line: usize, cell: f32) -> Div {
    let selected = doc
        .selection(view.id)
        .iter()
        .any(|range| range.cursor_line(doc.text().slice(..)) == line);
    let base = base_style(editor).patch(editor.theme.get(if selected {
        "ui.gutter.selected"
    } else {
        "ui.gutter"
    }));
    let mut row = div()
        .flex()
        .flex_shrink_0()
        .whitespace_nowrap()
        .w(px(view.gutter_offset(doc) as f32 * cell));
    for gutter_type in view.gutters() {
        let width = gutter_type.width(view, doc);
        let mut text = String::with_capacity(width);
        let mut render = gutter_type.style(editor, doc, view, &editor.theme, true);
        let style = base.patch(render(line, selected, true, &mut text).unwrap_or_default());
        let len = text.len();
        row = row.child(
            div()
                .w(px(width as f32 * cell))
                .flex_shrink_0()
                .bg(highlight(style).background_color.unwrap())
                .child(StyledText::new(text).with_highlights([(0..len, highlight(style))])),
        );
    }
    row
}

pub(super) fn statusline(editor: &Editor, doc: &Document, view: &View) -> Div {
    let base = highlight(base_style(editor).patch(editor.theme.get("ui.statusline")));
    let spinners = ui::ProgressSpinners::default();
    let mut context = ui::statusline::RenderContext::new(editor, doc, view, true, &spinners);
    ui::statusline::render_parts(&mut context);
    // Match helix-gpui's equal-width side containers. The center stays centered
    // even when the filename is much longer than the cursor position.
    div()
        .w_full()
        .flex_shrink_0()
        .flex()
        .font_family("JetBrains Mono")
        .text_size(px(14.))
        .line_height(gpui::phi())
        .bg(base.background_color.unwrap())
        .text_color(base.color.unwrap())
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(styled_spans(context.parts.left)),
        )
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .child(styled_spans(context.parts.center)),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .justify_end()
                .child(styled_spans(context.parts.right)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts_rgb_and_indexed_colors() {
        assert_eq!(
            color_to_hsla(Color::Rgb(12, 34, 56)),
            Some(rgb(0x0c2238).into())
        );
        assert_eq!(
            color_to_hsla(Color::Indexed(196)),
            Some(rgb(0xff0000).into())
        );
        assert_eq!(
            color_to_hsla(Color::Indexed(255)),
            Some(rgb(0xeeeeee).into())
        );
        assert_eq!(color_to_hsla(Color::Reset), None);
    }
    #[test]
    fn reversed_cursor_swaps_resolved_colors() {
        let style = highlight(
            Style::default()
                .fg(Color::White)
                .bg(Color::Black)
                .add_modifier(Modifier::REVERSED),
        );
        assert_eq!(style.color, color_to_hsla(Color::Black));
        assert_eq!(style.background_color, color_to_hsla(Color::White));
    }
}
