use helix_core::Position;
use helix_view::{
    editor::Editor,
    graphics::{Color, CursorKind, Modifier, UnderlineStyle},
};
use tui::buffer::Buffer;

/// Terminal cursor and editor-theme styling.
pub struct Style;

impl Style {
    pub fn cursor(buffer: &mut Buffer, position: Position, kind: CursorKind) {
        let Some(cell) = buffer.get_mut(position.col as u16, position.row as u16) else {
            return;
        };
        match kind {
            CursorKind::Block | CursorKind::Bar => cell.modifier.insert(Modifier::REVERSED),
            CursorKind::Underline => {
                cell.underline_style = UnderlineStyle::Line;
                cell.modifier.insert(Modifier::BOLD);
            }
            CursorKind::Hidden => {}
        }
    }

    pub fn background_css(editor: &Editor, dark: bool) -> String {
        let rgba = editor
            .theme
            .get("ui.background")
            .bg
            .and_then(Self::color_rgba)
            .unwrap_or(if dark {
                [0.0, 0.0, 0.0, 1.0]
            } else {
                [1.0, 1.0, 1.0, 1.0]
            });
        format!(
            "#{:02x}{:02x}{:02x}",
            (rgba[0] * 255.0).round() as u8,
            (rgba[1] * 255.0).round() as u8,
            (rgba[2] * 255.0).round() as u8,
        )
    }

    pub fn color_rgba(color: Color) -> Option<[f32; 4]> {
        let rgb = match color {
            Color::Rgb(red, green, blue) => [red, green, blue],
            Color::Black => [0, 0, 0],
            Color::Red => [205, 0, 0],
            Color::Green => [0, 205, 0],
            Color::Yellow => [205, 205, 0],
            Color::Blue => [0, 0, 238],
            Color::Magenta => [205, 0, 205],
            Color::Cyan => [0, 205, 205],
            Color::Gray => [128, 128, 128],
            Color::LightRed => [255, 0, 0],
            Color::LightGreen => [0, 255, 0],
            Color::LightBlue => [0, 0, 255],
            Color::LightYellow => [255, 255, 0],
            Color::LightMagenta => [255, 0, 255],
            Color::LightCyan => [0, 255, 255],
            Color::LightGray => [192, 192, 192],
            Color::White => [255, 255, 255],
            Color::Reset | Color::Indexed(_) => return None,
        };
        Some([
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            1.0,
        ])
    }
}
