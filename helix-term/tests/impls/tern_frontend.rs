use super::*;

fn preview_runtime(editor: &Editor) -> RenderRuntime {
    let context = RenderContext {
        body_font_size_px: 16.0,
        foreground_rgba: [0.0, 0.0, 0.0, 1.0],
        available_width_px: 640.0,
        source_root: std::path::PathBuf::from("."),
    };
    let document = DocumentPreview::new(current_rope(editor), context).unwrap();
    let mut renderer = TypstCliRenderer::new(std::path::PathBuf::from("typst"));
    renderer.load_settings(DEFAULT_SETTINGS).unwrap();
    RenderRuntime::new(document, renderer, UsvgProcessor::new())
}

#[tokio::test(flavor = "multi_thread")]
async fn cursor_navigation_synchronizes_block_activation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.typ");
    let source = "= Title\n\nA paragraph.\n\n```rust\nfn main() {}\n```\n";
    std::fs::write(&path, source).unwrap();
    let (mut editor, mut input, mut jobs) = create_session(Some(&path)).unwrap();
    let key = tern_sdk::Key {
        name: "i".into(),
        text: Some("i".into()),
        ..Default::default()
    };
    dispatch_key(&key, &mut editor, &mut input, &mut jobs);
    assert_eq!(editor.mode(), helix_view::document::Mode::Insert);
    dispatch_key(
        &tern_sdk::Key::named("escape"),
        &mut editor,
        &mut input,
        &mut jobs,
    );
    let mut runtime = preview_runtime(&editor);
    let ids: Vec<FragmentId> = runtime
        .document()
        .presentations()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids.len(), 1, "only the fenced block is previewable");
    let mut active = None;

    synchronize_cursor_fragment(&mut runtime, &editor, &mut active, false, false);
    assert_eq!(active, None, "initial caret must not activate a fragment");

    runtime.activate(ids[0]).unwrap();
    active = Some(ids[0]);
    select(&mut editor, source.find("A paragraph").unwrap());
    synchronize_cursor_fragment(&mut runtime, &editor, &mut active, true, false);
    assert_eq!(active, None, "ordinary text owns the caret");

    select(&mut editor, source.find("```rust").unwrap());
    synchronize_cursor_fragment(&mut runtime, &editor, &mut active, true, false);
    assert_eq!(active, Some(ids[0]), "the code block owns the caret");

    synchronize_cursor_fragment(&mut runtime, &editor, &mut active, true, true);
    assert_eq!(active, None, "focus loss keeps the block rendered");
}

#[test]
fn ansi_rows_preserve_helix_styles() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
    buffer.set_string(
        0,
        0,
        "x",
        helix_view::graphics::Style::default()
            .fg(Color::Red)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    );
    let rows = ansi_rows(&buffer).unwrap();
    assert!(rows[0].contains('x'));
    assert!(rows[0].contains("\u{1b}[1m"));
    assert!(rows[0].contains("38;5;1;48;5;4m"));
}

#[test]
fn visible_cursors_are_styled_in_the_helix_buffer() {
    let mut buffer = Buffer::with_lines(vec!["x"]);
    style_cursor(&mut buffer, Position::new(0, 0), CursorKind::Bar);
    assert!(buffer
        .get(0, 0)
        .unwrap()
        .modifier
        .contains(Modifier::REVERSED));
}

#[test]
fn hidden_or_blurred_surfaces_count_as_focus_loss() {
    assert!(is_focus_loss(&tern_sdk::Event::Visible(
        tern_sdk::wire::Visible {
            visible: false,
            ..Default::default()
        }
    )));
    assert!(is_focus_loss(&tern_sdk::Event::Resize(
        tern_sdk::wire::Resize {
            visible: Some(false),
            ..Default::default()
        }
    )));
    assert!(!is_focus_loss(&tern_sdk::Event::Visible(
        tern_sdk::wire::Visible {
            visible: true,
            ..Default::default()
        }
    )));

    let mut raw = serde_json::Map::new();
    raw.insert("ev".into(), "blur".into());
    assert!(is_focus_loss(&tern_sdk::Event::Unknown(raw)));
}

#[test]
fn tern_keys_map_to_helix_keys() {
    assert!(matches!(
        key_code(&tern_sdk::Key::named("page_up")),
        Some(KeyCode::PageUp)
    ));
    assert!(matches!(
        key_code(&tern_sdk::Key::named("f5")),
        Some(KeyCode::F(5))
    ));
}
