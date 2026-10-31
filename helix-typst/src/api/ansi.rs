use std::io;

use crossterm::{
    queue,
    style::{
        Attribute, Color as CrosstermColor, Colors, Print, SetAttribute, SetBackgroundColor,
        SetColors, SetForegroundColor, SetUnderlineColor,
    },
};
use helix_view::graphics::{Color, Modifier, UnderlineStyle};
use tui::buffer::{Buffer, Cell};

/// Mutable ANSI serialization state shared across one row.
pub struct AnsiWriter {
    output: Vec<u8>,
    foreground: Color,
    background: Color,
    underline_color: Color,
    underline_style: UnderlineStyle,
    modifier: Modifier,
}

impl AnsiWriter {
    fn new() -> Self {
        Self {
            output: Vec::new(),
            foreground: Color::Reset,
            background: Color::Reset,
            underline_color: Color::Reset,
            underline_style: UnderlineStyle::Reset,
            modifier: Modifier::empty(),
        }
    }

    pub fn rows(buffer: &Buffer) -> io::Result<Vec<String>> {
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(Self::row)
            .collect()
    }

    pub fn row(cells: &[Cell]) -> io::Result<String> {
        let mut writer = Self::new();

        for cell in cells {
            // Write one cell, emitting only the style changes needed since the previous cell.
            if cell.modifier != writer.modifier {
                queue!(writer.output, SetAttribute(Attribute::Reset))?;
                writer.modifier = cell.modifier;
                let flags = [
                    (Modifier::BOLD, Attribute::Bold),
                    (Modifier::DIM, Attribute::Dim),
                    (Modifier::ITALIC, Attribute::Italic),
                    (Modifier::SLOW_BLINK, Attribute::SlowBlink),
                    (Modifier::RAPID_BLINK, Attribute::RapidBlink),
                    (Modifier::REVERSED, Attribute::Reverse),
                    (Modifier::HIDDEN, Attribute::Hidden),
                    (Modifier::CROSSED_OUT, Attribute::CrossedOut),
                ];
                for (flag, attribute) in flags {
                    if writer.modifier.contains(flag) {
                        queue!(writer.output, SetAttribute(attribute))?;
                    }
                }
                writer.foreground = Color::Reset;
                writer.background = Color::Reset;
                writer.underline_color = Color::Reset;
                writer.underline_style = UnderlineStyle::Reset;
            }

            if cell.fg != writer.foreground || cell.bg != writer.background {
                queue!(
                    writer.output,
                    SetColors(Colors::new(
                        Self::crossterm_color(cell.fg),
                        Self::crossterm_color(cell.bg),
                    ))
                )?;
                writer.foreground = cell.fg;
                writer.background = cell.bg;
            }

            if cell.underline_color != writer.underline_color {
                queue!(
                    writer.output,
                    SetUnderlineColor(Self::crossterm_color(cell.underline_color))
                )?;
                writer.underline_color = cell.underline_color;
            }

            if cell.underline_style != writer.underline_style {
                queue!(
                    writer.output,
                    SetAttribute(match cell.underline_style {
                        UnderlineStyle::Line => Attribute::Underlined,
                        UnderlineStyle::Curl => Attribute::Undercurled,
                        UnderlineStyle::Dotted => Attribute::Underdotted,
                        UnderlineStyle::Dashed => Attribute::Underdashed,
                        UnderlineStyle::DoubleLine => Attribute::DoubleUnderlined,
                        UnderlineStyle::Reset => Attribute::NoUnderline,
                    })
                )?;
                writer.underline_style = cell.underline_style;
            }

            queue!(writer.output, Print(&cell.symbol))?;
        }

        queue!(
            writer.output,
            SetUnderlineColor(CrosstermColor::Reset),
            SetForegroundColor(CrosstermColor::Reset),
            SetBackgroundColor(CrosstermColor::Reset),
            SetAttribute(Attribute::Reset)
        )?;

        Ok(String::from_utf8(writer.output).expect("crossterm emits valid UTF-8"))
    }

    fn crossterm_color(color: Color) -> CrosstermColor {
        match color {
            Color::Reset => CrosstermColor::Reset,
            Color::Black => CrosstermColor::Black,
            Color::Red => CrosstermColor::DarkRed,
            Color::Green => CrosstermColor::DarkGreen,
            Color::Yellow => CrosstermColor::DarkYellow,
            Color::Blue => CrosstermColor::DarkBlue,
            Color::Magenta => CrosstermColor::DarkMagenta,
            Color::Cyan => CrosstermColor::DarkCyan,
            Color::Gray => CrosstermColor::DarkGrey,
            Color::LightRed => CrosstermColor::Red,
            Color::LightGreen => CrosstermColor::Green,
            Color::LightBlue => CrosstermColor::Blue,
            Color::LightYellow => CrosstermColor::Yellow,
            Color::LightMagenta => CrosstermColor::Magenta,
            Color::LightCyan => CrosstermColor::Cyan,
            Color::LightGray => CrosstermColor::Grey,
            Color::White => CrosstermColor::White,
            Color::Indexed(index) => CrosstermColor::AnsiValue(index),
            Color::Rgb(red, green, blue) => CrosstermColor::Rgb {
                r: red,
                g: green,
                b: blue,
            },
        }
    }
}
