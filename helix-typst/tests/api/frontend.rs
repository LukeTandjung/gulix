use super::super::{active::ActiveFragment, ansi::AnsiWriter, session::HelixSession, style::Style};
use helix_view::{
    graphics::{Color, CursorKind, Modifier, Rect},
    input::KeyCode,
};
use quickcheck::{quickcheck, TestResult};
use tui::buffer::Buffer;

use crate::{DocumentPreview, RenderContext};

fn preview_document(session: &HelixSession) -> DocumentPreview {
    let context = RenderContext {
        body_font_size_px: 16.0,
        foreground_rgba: [0.0, 0.0, 0.0, 1.0],
        available_width_px: 640.0,
        source_root: std::path::PathBuf::from("."),
    };
    DocumentPreview::new(session.current_rope(), context).unwrap()
}

fn insert_mode_key() -> tern_sdk::Key {
    tern_sdk::Key {
        name: "i".into(),
        text: Some("i".into()),
        ..Default::default()
    }
}

quickcheck! {
    fn cursor_position_determines_active_fragment(cursor: usize) -> TestResult {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.typ");
        let source = "= Title\n\nA paragraph.\n\n```rust\nfn main() {}\n```\n";
        std::fs::write(&path, source).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _runtime_context = runtime.enter();
        let mut helix = HelixSession::new(Some(&path)).unwrap();
        helix.dispatch_key(&insert_mode_key());
        helix.dispatch_key(&tern_sdk::Key::named("escape"));
        let mut document = preview_document(&helix);
        let id = document
            .presentations(None)
            .expect("block presentation")
            .map(|(id, _)| id)
            .next()
            .expect("code block");
        let mut active = ActiveFragment::new();
        let position = cursor % (source.chars().count() + 1);
        helix.select(position);
        active.synchronize(&mut document, &helix, false);
        let start = source.find("```rust").unwrap();
        let end = start + "```rust\nfn main() {}\n```".chars().count();
        let expected = position > start && position < end;

        TestResult::from_bool(
            active.active() == expected.then_some(id)
                && {
                    active.suspend(&mut document);
                    active.active().is_none()
                },
        )
    }

    fn ansi_output_preserves_generated_text_and_styles(
        character: u8,
        red: u8,
        green: u8,
        blue: u8,
        bold: bool
    ) -> TestResult {
        let character = char::from(b'a' + character % 26);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        let mut style = helix_view::graphics::Style::default()
            .fg(Color::Rgb(red, green, blue))
            .bg(Color::Rgb(blue, green, red));
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        buffer.set_string(0, 0, &character.to_string(), style);
        let rows = AnsiWriter::rows(&buffer).unwrap();
        let style_preserved = rows[0].contains("38;2;") && rows[0].contains("48;2;");
        let bold_preserved = rows[0].contains("\u{1b}[1m") == bold;

        TestResult::from_bool(
            rows[0].contains(character) && style_preserved && bold_preserved,
        )
    }

    fn cursor_kinds_apply_their_expected_modifiers(kind: u8) -> TestResult {
        let kind = match kind % 4 {
            0 => CursorKind::Block,
            1 => CursorKind::Bar,
            2 => CursorKind::Underline,
            _ => CursorKind::Hidden,
        };
        let mut buffer = Buffer::with_lines(vec!["x"]);
        Style::cursor(&mut buffer, helix_core::Position::new(0, 0), kind);
        let cell = buffer.get(0, 0).unwrap();
        let valid = match kind {
            CursorKind::Block | CursorKind::Bar => cell.modifier.contains(Modifier::REVERSED),
            CursorKind::Underline => {
                cell.modifier.contains(Modifier::BOLD)
                    && cell.underline_style == helix_view::graphics::UnderlineStyle::Line
            }
            CursorKind::Hidden => cell.modifier.is_empty(),
        };

        TestResult::from_bool(valid)
    }

    fn named_keys_and_characters_map_to_helix_keys(index: u8, function: u8, character: u8) -> TestResult {
        const NAMES: &[&str] = &[
            "escape", "enter", "backspace", "delete", "insert", "tab", "left", "right",
            "up", "down", "home", "end", "page_up", "page_down",
        ];
        let name = NAMES[index as usize % NAMES.len()];
        let named = HelixSession::key_code(&tern_sdk::Key::named(name));
        let function = HelixSession::key_code(&tern_sdk::Key::named(format!(
            "f{}",
            1 + function % 12
        )));
        let character = char::from(b'a' + character % 26);
        let character = HelixSession::key_code(&tern_sdk::Key {
            name: character.to_string(),
            text: Some(character.to_string()),
            ..Default::default()
        });

        TestResult::from_bool(
            named.is_some()
                && matches!(function, Some(KeyCode::F(_)))
                && matches!(character, Some(KeyCode::Char(_))),
        )
    }
}
