//! Tern Surface Protocol frontend for inline Typst block previews.
//!
//! Helix owns the source, input, cursor, selections, and ordinary editor UI.
//! Tern displays Helix's compositor rows and substitutes only rendered blocks.

use std::{collections::BTreeMap, io, ops::Range, path::Path, sync::Arc, time::Duration};

use arc_swap::{access::Map, ArcSwap};
use crossterm::{
    queue,
    style::{
        Attribute, Color as CrosstermColor, Colors, Print, SetAttribute, SetBackgroundColor,
        SetColors, SetForegroundColor, SetUnderlineColor,
    },
};
use helix_core::{syntax, visual_offset_from_block, Position, Rope, Selection};
use helix_loader::workspace_trust::WorkspaceTrust;
use helix_view::{
    editor::{Action, Editor, GutterType},
    graphics::{Color, CursorKind, Modifier, Rect, UnderlineStyle},
    gutter::GutterFn,
    input::{Event, KeyCode, KeyEvent, KeyModifiers},
    theme,
};
use tern_sdk::{
    ui::{self, Bound, Extent, Node, View},
    Input, Options, Session, Surface, SurfaceOptions,
};
use tui::buffer::{Buffer, Cell};

use crate::{
    compositor::{Compositor, Context as InputContext},
    config::Config,
    job::Jobs,
    keymap::Keymaps,
    typst_preview::{
        DocumentPreview, FragmentId, FragmentKind, Presentation, RenderContext, RenderRuntime,
        RenderedFragment, TerminalLayout, TypstCliRenderer, TypstRenderer, UsvgProcessor,
        DEFAULT_SETTINGS,
    },
    ui::{EditorView, Prompt},
};

#[derive(Clone, Debug)]
enum Msg {
    Reveal(FragmentId),
    Deactivate,
}

/// Creates the Helix session owned by the Tern frontend, leaving LSP diagnostics enabled.
fn create_session(path: Option<&Path>) -> anyhow::Result<(Editor, Compositor, Jobs)> {
    helix_loader::initialize_config_file(None);
    let loaded = match Config::load_default() {
        Ok(config) => config,
        Err(crate::config::ConfigLoadError::Error(error))
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            Config::default()
        }
        Err(error) => anyhow::bail!("Failed to load Helix configuration: {error}"),
    };
    let trust = WorkspaceTrust::new((&loaded.editor.workspace_trust).into());
    let config = Arc::new(ArcSwap::from_pointee(loaded));
    let handlers = crate::handlers::setup(config.clone());
    let mut theme_dirs = vec![helix_loader::config_dir()];
    theme_dirs.extend(helix_loader::runtime_dirs().iter().cloned());
    let theme_loader = Arc::new(theme::Loader::new(&theme_dirs));
    let selected_theme = match &config.load().theme {
        Some(selected) => theme_loader.load(selected.choose(None))?,
        None => theme_loader.default_theme(true),
    };
    let languages = helix_core::config::user_lang_config(&trust)?;
    let syntax = syntax::Loader::new(languages)?;
    let area = Rect::new(0, 0, 120, 40);
    let mut editor = Editor::new(
        area,
        theme_loader,
        Arc::new(ArcSwap::from_pointee(syntax)),
        Arc::new(Map::new(config.clone(), |config: &Config| &config.editor)),
        handlers,
        trust,
    );
    editor.set_theme(selected_theme)?;
    match path {
        Some(path) => {
            editor.open(path, Action::VerticalSplit)?;
        }
        None => {
            editor.new_file(Action::VerticalSplit);
        }
    }
    let mut input = Compositor::new(area);
    input.push(Box::new(EditorView::new(Keymaps::new(Box::new(Map::new(
        config,
        |config: &Config| &config.keys,
    ))))));
    Ok((editor, input, Jobs::new()))
}

pub fn run(path: Option<&Path>) -> anyhow::Result<()> {
    let Some(mut session) = Session::<Msg>::connect(Options::new().app("gulix"))? else {
        println!("hxt needs to run inside Tern; use hx for the terminal frontend.");
        return Ok(());
    };
    let (mut editor, mut input, mut jobs) = create_session(path)?;
    resize_editor(&mut editor, &mut input, session.caps().cols);
    let source_root = path
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let context = render_context(&session, source_root, &editor);
    let document = DocumentPreview::new(current_rope(&editor), context.clone())
        .map_err(|error| anyhow::anyhow!("invalid initial fragments: {error:?}"))?;
    let mut renderer = TypstCliRenderer::new(Path::new("typst").to_path_buf());
    renderer
        .load_settings(DEFAULT_SETTINGS)
        .map_err(|error| anyhow::anyhow!("failed to load Typst fonts: {error:?}"))?;
    let mut runtime = RenderRuntime::new(document, renderer, UsvgProcessor::new());
    let surface = session.open(
        SurfaceOptions::screen()
            .title("Gulix Typst")
            .role("gulix.typst"),
    )?;
    let background_css = format!(
        ".sf-region, .sf-rows, .sf-row, .sf-col, .sf-status {{ background-color: {} !important; }} .sf-rows, .sf-row, .sf-status {{ width: 100% !important; max-width: none !important; margin: 0 !important; }} .sf-rows[data-role='gulix-inline-text'] {{ width: auto !important; }} .sf-col[data-role='gulix-status'] {{ background-color: transparent !important; }} .sf-rows-mark {{ display: none !important; }}",
        editor_background_css(&editor, session.caps().dark)
    );
    // Fill region gaps from the first frame. Otherwise the page background
    // shows through between the editor and the raised chrome, most visibly
    // as a light horizontal strip under an open command prompt.
    session.stylesheet(surface, "gulix", Some(&background_css))?;

    let mut active = None;
    let mut preview_suspended = false;
    let mut synchronize_initial = true;
    let mut revealed_initial = false;
    let mut initial_reveal_pending = false;
    let mut context = context;

    loop {
        let next_context = render_context(&session, context.source_root.clone(), &editor);
        if next_context != context {
            context = next_context;
        }
        runtime.schedule_missing(context.clone());
        runtime.poll();
        synchronize_cursor_fragment(
            &mut runtime,
            &editor,
            &mut active,
            !synchronize_initial,
            preview_suspended,
        );
        synchronize_initial = false;
        render(
            &mut session,
            surface,
            &mut editor,
            &mut input,
            &mut jobs,
            &mut runtime,
            active,
        )?;
        while let Ok(callback) = jobs.callbacks.try_recv() {
            if let Some(job) = jobs.handle_callback(&mut editor, &mut input, Ok(Some(callback))) {
                jobs.add(job);
            }
        }

        let Some(event) = session.next(Some(Duration::from_millis(50)))? else {
            if initial_reveal_pending {
                session.scroll(surface, "main", tern_sdk::wire::ScrollBy::Start)?;
                revealed_initial = true;
                initial_reveal_pending = false;
            }
            continue;
        };
        match event {
            Input::Key(key) if key.is("ctrl+c") => break,
            Input::Key(key) => {
                preview_suspended = false;
                let previous_source = current_rope(&editor);
                dispatch_key(&key, &mut editor, &mut input, &mut jobs);
                let source = current_rope(&editor);
                if source != previous_source {
                    runtime
                        .replace_source(source)
                        .map_err(|error| anyhow::anyhow!("invalid edited fragments: {error:?}"))?;
                    if active.is_some_and(|_| runtime.presentations(active).is_none()) {
                        active = None;
                    }
                }
                if editor.should_close() {
                    break;
                }
            }
            Input::Msg(Msg::Reveal(id), _) => {
                preview_suspended = false;
                if let Some(presentation) = runtime.activate(id) {
                    select(&mut editor, presentation_range(&presentation).start);
                    active = Some(id);
                }
            }
            Input::Msg(Msg::Deactivate, _) => {
                preview_suspended = false;
                if active.take().is_some() {
                    runtime.deactivate();
                }
            }
            Input::Event(event) => {
                if !revealed_initial && matches!(event, tern_sdk::Event::Resize(_)) {
                    initial_reveal_pending = true;
                }
                if is_focus_loss(&event) {
                    preview_suspended = true;
                    if active.take().is_some() {
                        runtime.deactivate();
                    }
                }
                if is_focus_gain(&event) {
                    preview_suspended = false;
                }
                resize_editor(&mut editor, &mut input, session.caps().cols);
            }
        }
    }
    session.close()?;
    Ok(())
}

fn is_focus_loss(event: &tern_sdk::Event) -> bool {
    match event {
        tern_sdk::Event::Visible(event) => !event.visible,
        tern_sdk::Event::Resize(event) => event.visible == Some(false),
        _ => matches!(event.name(), "blur" | "focusout" | "deactivate"),
    }
}

fn is_focus_gain(event: &tern_sdk::Event) -> bool {
    match event {
        tern_sdk::Event::Visible(event) => event.visible,
        _ => matches!(event.name(), "focus" | "focusin" | "activate"),
    }
}

fn render_context(
    session: &Session<Msg>,
    source_root: std::path::PathBuf,
    editor: &Editor,
) -> RenderContext {
    let caps = session.caps();
    let cell = caps.cell.unwrap_or_default();
    let cell_width = if cell.w > 0.0 { cell.w as f32 } else { 8.0 };
    let fallback = if caps.dark {
        [1.0, 1.0, 1.0, 1.0]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    RenderContext {
        body_font_size_px: if cell.h > 0.0 { cell.h as f32 } else { 16.0 },
        foreground_rgba: editor
            .theme
            .get("ui.text")
            .fg
            .and_then(color_rgba)
            .unwrap_or(fallback),
        available_width_px: caps.cols.saturating_sub(8).max(1) as f32 * cell_width,
        source_root,
    }
}

fn editor_background_css(editor: &Editor, dark: bool) -> String {
    let rgba = editor
        .theme
        .get("ui.background")
        .bg
        .and_then(color_rgba)
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

fn color_rgba(color: Color) -> Option<[f32; 4]> {
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

fn synchronize_cursor_fragment(
    runtime: &mut RenderRuntime,
    editor: &Editor,
    active: &mut Option<FragmentId>,
    allow_start: bool,
    suspended: bool,
) {
    if suspended {
        if active.take().is_some() {
            runtime.deactivate();
        }
        return;
    }
    let cursor = current_cursor(editor);
    let navigating = active.is_some() || allow_start;
    if let Some(id) = *active {
        let contains_cursor = runtime
            .presentations(*active)
            .and_then(|mut presentations| presentations.find(|(fragment, _)| *fragment == id))
            .is_some_and(|(_, presentation)| {
                let range = presentation_range(&presentation);
                cursor >= range.start && cursor <= range.end
            });
        if contains_cursor {
            return;
        }
        runtime.deactivate();
        *active = None;
    }
    let containing = runtime
        .presentations(*active)
        .into_iter()
        .flatten()
        .find_map(|(id, presentation)| {
            let range = presentation_range(&presentation);
            let contains = if navigating {
                cursor >= range.start && cursor < range.end
            } else {
                cursor > range.start && cursor < range.end
            };
            contains.then_some(id)
        });
    if let Some(id) = containing {
        if runtime.activate(id).is_some() {
            *active = Some(id);
        }
    }
}

fn presentation_range<'a>(presentation: &'a Presentation<'_>) -> &'a Range<usize> {
    match presentation {
        Presentation::Source { range, .. } | Presentation::Rendered { range, .. } => range,
    }
}

struct PreviewLayout {
    id: FragmentId,
    kind: FragmentKind,
    rendered: RenderedFragment,
    start_row: usize,
    end_row: usize,
    start_line: usize,
    end_line: usize,
}

struct InlineLayout {
    id: FragmentId,
    rendered: RenderedFragment,
    row: usize,
    start_col: usize,
    end_col: usize,
}

fn render(
    session: &mut Session<Msg>,
    surface: Surface,
    editor: &mut Editor,
    input: &mut Compositor,
    jobs: &mut Jobs,
    runtime: &mut RenderRuntime,
    active: Option<FragmentId>,
) -> anyhow::Result<()> {
    let area = input.size();
    let mut buffer = Buffer::empty(area);
    input.render(
        area,
        &mut buffer,
        &mut InputContext {
            editor,
            jobs,
            scroll: None,
        },
    );
    let modal_open = input.layer_count() > 1
        || editor.autoinfo.is_some()
        || input
            .find::<EditorView>()
            .is_some_and(|view| view.completion.is_some());
    let prompt_open = input.find::<Prompt>().is_some();
    let (cursor, cursor_kind) = input.cursor(area, editor);
    editor.cursor_cache.reset();
    if let Some(cursor) = cursor {
        style_cursor(&mut buffer, cursor, cursor_kind);
    }
    let rows = ansi_rows(&buffer)?;
    let columns = area.width as u32;
    // Helix pins its status line second-to-last and its command/pending-key
    // line last inside whatever area it renders; `TerminalLayout` mirrors that
    // placement (and `ui::overlay::overlaid`'s dialog clearance) so pickers and
    // dialogs land exactly as in plain Helix. Prompts are different: their
    // completions may cover the status-line row, so all rows must stay in one
    // continuous main region rather than splitting the menu around the spacer.
    let main_height = if prompt_open {
        rows.len()
    } else {
        TerminalLayout::new(area.height).map_or(area.height.saturating_sub(2) as usize, |layout| {
            layout.main_region_rows(prompt_open) as usize
        })
    };
    let cell_height_px = {
        let height = session.caps().cell.unwrap_or_default().h;
        if height > 0.0 {
            height
        } else {
            16.0
        }
    };
    let view = editor.tree.get(editor.tree.focus);
    let doc = editor.document(view.doc).unwrap();
    let inner = view.inner_area(doc);
    let view_offset = doc.view_offset(view.id);
    let text = doc.text().slice(..);
    let text_fmt = doc.text_format(inner.width, None);
    let annotations = view.text_annotations(doc, None);
    let visual_position = |position| {
        visual_offset_from_block(text, view_offset.anchor, position, &text_fmt, &annotations).0
    };
    let mut layouts: Vec<PreviewLayout> = runtime
        .presentations(active)
        .into_iter()
        .flatten()
        .filter_map(|(id, presentation)| {
            if modal_open || active == Some(id) {
                return None;
            }
            let Presentation::Rendered {
                kind,
                rendered,
                range,
                ..
            } = presentation
            else {
                return None;
            };
            if kind == FragmentKind::InlineMath {
                return None;
            }
            Some(PreviewLayout {
                id,
                kind,
                rendered: rendered.clone(),
                start_row: visual_position(range.start).row,
                end_row: visual_position(range.end.saturating_sub(1)).row,
                start_line: text.char_to_line(range.start),
                end_line: text.char_to_line(range.end.saturating_sub(1)),
            })
        })
        .collect();
    layouts.sort_by_key(|layout| layout.start_row);

    let viewport_start = view.area.y as isize;
    let viewport_end = (viewport_start + view.area.height as isize).min(main_height as isize);
    let vertical_offset = view_offset.vertical_offset as isize;
    let horizontal_offset = view_offset.horizontal_offset as isize;
    let mut inline_layouts: Vec<InlineLayout> = runtime
        .presentations(active)
        .into_iter()
        .flatten()
        .filter_map(|(id, presentation)| {
            if modal_open || active == Some(id) {
                return None;
            }
            let Presentation::Rendered {
                kind: FragmentKind::InlineMath,
                rendered,
                range,
                ..
            } = presentation
            else {
                return None;
            };
            let start = visual_position(range.start);
            let end = visual_position(range.end);
            if start.row != end.row {
                return None;
            }
            let row = viewport_start + start.row as isize - vertical_offset;
            let start_col = inner.x as isize + start.col as isize - horizontal_offset;
            let end_col = inner.x as isize + end.col as isize - horizontal_offset;
            if row < viewport_start
                || row >= viewport_end
                || start_col < inner.x as isize
                || end_col > inner.x as isize + inner.width as isize
            {
                return None;
            }
            Some(InlineLayout {
                id,
                rendered: rendered.clone(),
                row: row as usize,
                start_col: start_col as usize,
                end_col: end_col as usize,
            })
        })
        .collect();
    inline_layouts.sort_by_key(|layout| (layout.row, layout.start_col));
    let mut inline_by_row: BTreeMap<usize, Vec<InlineLayout>> = BTreeMap::new();
    for layout in inline_layouts {
        inline_by_row.entry(layout.row).or_default().push(layout);
    }
    let mut inline_nodes = BTreeMap::new();
    for (row, layouts) in inline_by_row {
        inline_nodes.insert(
            row,
            inline_row(session, &buffer, row, &layouts, cell_height_px)?,
        );
    }

    let mut children = Vec::new();
    let mut line_numbers = GutterType::LineNumbers.style(editor, doc, view, &editor.theme, true);
    let linenr_trailing = view
        .gutters
        .layout
        .iter()
        .rev()
        .take_while(|gutter| !matches!(gutter, GutterType::LineNumbers))
        .count() as u16;
    // A preview replaces source rows with a vector node whose height need not
    // be a whole multiple of the row height, and image-backed nodes must not
    // be shrunk to a single row (Tern drops the image to the row bottom when
    // squeezed), so the column can grow taller than the terminal. Track the
    // height consumed so far and stop filling the column once the chrome would
    // be covered: overflowing rows would draw underneath the pinned status
    // area, which is exactly the encroachment `TerminalLayout` rules out.
    let row_budget_px = main_height as f64 * cell_height_px;
    let mut consumed_px = 0.0;
    let mut row = 0;
    for layout in layouts {
        let start = viewport_start + layout.start_row as isize - vertical_offset;
        let end = viewport_start + layout.end_row as isize - vertical_offset;
        if end < viewport_start || start >= viewport_end {
            continue;
        }
        let replace_start = start.max(viewport_start).max(0) as usize;
        let replace_end = end.min(viewport_end - 1) as usize;
        if replace_start < row || replace_start > replace_end {
            continue;
        }
        if row < replace_start {
            push_rows(
                &mut children,
                row,
                replace_start,
                &rows,
                columns,
                &inline_nodes,
            );
            consumed_px += (replace_start - row) as f64 * cell_height_px;
        }
        let preview_height_px = layout.rendered.intrinsic_height_px.ceil().max(1.0) as f64;
        children.push(preview_node(
            session,
            &layout,
            inner.x,
            &mut line_numbers,
            linenr_trailing,
            inner.width,
            cell_height_px,
        )?);
        consumed_px += preview_height_px;
        row = replace_end + 1;
    }
    if row < main_height && consumed_px < row_budget_px {
        let remaining_rows = ((row_budget_px - consumed_px) / cell_height_px).floor() as usize;
        let fill_end = row.saturating_add(remaining_rows).min(main_height);
        if row < fill_end {
            push_rows(&mut children, row, fill_end, &rows, columns, &inline_nodes);
        }
    }

    // Chrome layer: any genuine status/command rows left outside the main
    // region, pinned above one trailing spacer row. An open prompt owns those
    // rows in the main region, so only the spacer remains here.
    session.render(
        surface,
        View::new()
            .main(
                ui::col().child(
                    ui::col()
                        .gap(ui::Gap::None)
                        .min(Bound::h(Extent::Fraction(1.0)))
                        .grow(1.0)
                        .children(children),
                ),
            )
            .layer(
                ui::col().child(
                    ui::col()
                        .role("gulix-status")
                        .min(Bound::h(Extent::Fraction(1.0)))
                        .justify(ui::Justify::End)
                        .children(
                            rows[main_height..]
                                .iter()
                                .map(|row| Node::from(ui::ansi(row.clone()).cols(columns)))
                                .chain(std::iter::once(Node::from(
                                    ui::spacer().min(Bound::h(Extent::Lines(1.0))),
                                ))),
                        ),
                ),
            ),
    )?;
    Ok(())
}

fn push_rows(
    children: &mut Vec<Node<Msg>>,
    start: usize,
    end: usize,
    rows: &[String],
    columns: u32,
    inline_nodes: &BTreeMap<usize, Node<Msg>>,
) {
    for (row, source) in rows.iter().enumerate().skip(start).take(end - start) {
        if let Some(node) = inline_nodes.get(&row) {
            children.push(node.clone());
        } else {
            children.push(
                ui::rows(vec![source.clone()])
                    .cols(columns)
                    .on_click(Msg::Deactivate)
                    .into(),
            );
        }
    }
}

fn inline_row(
    session: &mut Session<Msg>,
    buffer: &Buffer,
    row: usize,
    layouts: &[InlineLayout],
    cell_height_px: f64,
) -> anyhow::Result<Node<Msg>> {
    let width = buffer.area.width as usize;
    let cells = &buffer.content()[row * width..(row + 1) * width];
    let mut children = Vec::new();
    let mut column = 0;
    for layout in layouts {
        if column < layout.start_col {
            children.push(ansi_segment(
                &cells[column..layout.start_col],
                layout.start_col - column,
            )?);
        }
        let blob = session.blob(
            layout.rendered.svg.clone().into_bytes(),
            Some("image/svg+xml"),
        )?;
        // Tern renders the SVG at its natural size, so the layout width must match
        // it exactly: a wider box (for example one scaled by the cell-height
        // ratio) leaves dead space after the glyphs. The height prop must stay a
        // full line, though: with a smaller natural height Tern drops the image
        // to the row bottom instead of aligning its baseline with the text.
        let width_px = f64::from(layout.rendered.intrinsic_width_px).max(1.0);
        children.push(
            ui::image()
                .key(format!("fragment-{}", layout.id.0))
                .blob(blob)
                .w(width_px)
                .h(cell_height_px.ceil())
                .min(Bound::h(Extent::Lines(1.0)))
                .max(Bound::h(Extent::Lines(1.0)))
                .grow(0.0)
                .shrink(0.0)
                .basis(ui::Basis::Content)
                .alt("Inline math. Click to edit its source.")
                .on_click(Msg::Reveal(layout.id))
                .into(),
        );
        column = layout.end_col;
    }
    if column < width {
        children.push(ansi_segment(&cells[column..], width - column)?);
    }
    Ok(ui::row()
        .gap(ui::Gap::None)
        .align(ui::Align::Baseline)
        .children(children)
        .into())
}

fn ansi_segment(cells: &[Cell], columns: usize) -> anyhow::Result<Node<Msg>> {
    Ok(ui::rows(vec![ansi_row(cells)?])
        .role("gulix-inline-text")
        .cols(columns as u32)
        .min(Bound::w(Extent::Ch(columns as f64)))
        .max(Bound::w(Extent::Ch(columns as f64)))
        .grow(0.0)
        .shrink(0.0)
        .basis(ui::Basis::Content)
        .on_click(Msg::Deactivate)
        .into())
}

fn preview_node(
    session: &mut Session<Msg>,
    layout: &PreviewLayout,
    inner_x: u16,
    line_numbers: &mut GutterFn<'_>,
    linenr_trailing: u16,
    inner_width: u16,
    cell_height_px: f64,
) -> anyhow::Result<Node<Msg>> {
    let PreviewLayout {
        id,
        kind,
        rendered,
        start_line,
        end_line,
        ..
    } = layout;
    let blob = session.blob(rendered.svg.clone().into_bytes(), Some("image/svg+xml"))?;
    let height_px = f64::from(rendered.intrinsic_height_px).ceil().max(1.0);
    let image: Node<Msg> = ui::image()
        .key(format!("fragment-{}", id.0))
        .blob(blob)
        .w(f64::from(rendered.intrinsic_width_px).ceil().max(1.0))
        .h(height_px)
        .grow(0.0)
        .shrink(0.0)
        .basis(ui::Basis::Content)
        .alt("Typst block. Click to edit its source.")
        .on_click(Msg::Reveal(*id))
        .into();
    let mut gutter = ui::col()
        .min(Bound::wh(
            Extent::Ch(inner_x as f64),
            Extent::Lines(height_px / cell_height_px),
        ))
        .max(Bound::w(Extent::Ch(inner_x as f64)))
        .justify(ui::Justify::Between)
        .grow(0.0)
        .shrink(0.0)
        .child(line_number_node(
            *start_line,
            inner_x,
            line_numbers,
            linenr_trailing,
        )?);
    if end_line != start_line {
        gutter = gutter.child(line_number_node(
            *end_line,
            inner_x,
            line_numbers,
            linenr_trailing,
        )?);
    }
    if matches!(kind, FragmentKind::DisplayMath | FragmentKind::Figure) {
        let column = ui::col()
            .align(ui::Align::Center)
            .min(Bound::w(Extent::Ch(inner_width as f64)))
            .child(image);
        Ok(ui::row().child(gutter).child(column).into())
    } else {
        Ok(ui::row().child(gutter).child(image).into())
    }
}

fn line_number_node(
    line: usize,
    gutter_width: u16,
    line_numbers: &mut GutterFn<'_>,
    linenr_trailing: u16,
) -> anyhow::Result<Node<Msg>> {
    let mut label = String::new();
    let style = line_numbers(line, false, true, &mut label).unwrap_or_default();
    let mut buffer = Buffer::empty(Rect::new(0, 0, gutter_width.max(1), 1));
    // Helix pads the label to the line-number gutter's width. Place that whole
    // gutter after preceding gutters and before trailing configured gutters.
    let label_width = label.chars().count() as u16;
    let x = gutter_width
        .saturating_sub(linenr_trailing)
        .saturating_sub(label_width);
    buffer.set_string(x, 0, &label, style);
    Ok(ui::ansi(ansi_row(buffer.content())?)
        .cols(gutter_width as u32)
        .min(Bound::wh(
            Extent::Ch(gutter_width as f64),
            Extent::Lines(1.0),
        ))
        .max(Bound::wh(
            Extent::Ch(gutter_width as f64),
            Extent::Lines(1.0),
        ))
        .grow(0.0)
        .shrink(0.0)
        .into())
}

fn style_cursor(buffer: &mut Buffer, position: Position, kind: CursorKind) {
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

fn ansi_rows(buffer: &Buffer) -> io::Result<Vec<String>> {
    buffer
        .content()
        .chunks(buffer.area.width as usize)
        .map(ansi_row)
        .collect()
}

fn ansi_row(cells: &[Cell]) -> io::Result<String> {
    let mut output = Vec::new();
    let mut foreground = Color::Reset;
    let mut background = Color::Reset;
    let mut underline_color = Color::Reset;
    let mut underline_style = UnderlineStyle::Reset;
    let mut modifier = Modifier::empty();
    for cell in cells {
        if cell.modifier != modifier {
            queue!(output, SetAttribute(Attribute::Reset))?;
            modifier = cell.modifier;
            queue_modifier(&mut output, modifier)?;
            foreground = Color::Reset;
            background = Color::Reset;
            underline_color = Color::Reset;
            underline_style = UnderlineStyle::Reset;
        }
        if cell.fg != foreground || cell.bg != background {
            queue!(
                output,
                SetColors(Colors::new(
                    crossterm_color(cell.fg),
                    crossterm_color(cell.bg),
                ))
            )?;
            foreground = cell.fg;
            background = cell.bg;
        }
        if cell.underline_color != underline_color {
            queue!(
                output,
                SetUnderlineColor(crossterm_color(cell.underline_color))
            )?;
            underline_color = cell.underline_color;
        }
        if cell.underline_style != underline_style {
            queue!(
                output,
                SetAttribute(crossterm_underline(cell.underline_style))
            )?;
            underline_style = cell.underline_style;
        }
        queue!(output, Print(&cell.symbol))?;
    }
    queue!(
        output,
        SetUnderlineColor(CrosstermColor::Reset),
        SetForegroundColor(CrosstermColor::Reset),
        SetBackgroundColor(CrosstermColor::Reset),
        SetAttribute(Attribute::Reset)
    )?;
    Ok(String::from_utf8(output).expect("crossterm emits valid UTF-8"))
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

fn crossterm_underline(style: UnderlineStyle) -> Attribute {
    match style {
        UnderlineStyle::Line => Attribute::Underlined,
        UnderlineStyle::Curl => Attribute::Undercurled,
        UnderlineStyle::Dotted => Attribute::Underdotted,
        UnderlineStyle::Dashed => Attribute::Underdashed,
        UnderlineStyle::DoubleLine => Attribute::DoubleUnderlined,
        UnderlineStyle::Reset => Attribute::NoUnderline,
    }
}

fn queue_modifier(output: &mut Vec<u8>, modifier: Modifier) -> io::Result<()> {
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
        if modifier.contains(flag) {
            queue!(output, SetAttribute(attribute))?;
        }
    }
    Ok(())
}

fn resize_editor(editor: &mut Editor, input: &mut Compositor, columns: u32) {
    // Tern's pty covers the whole surface, but the frontend renders Helix into
    // a virtual terminal one row shorter: the freed surface row becomes the
    // raised spacer below the command line (see `render`). Inside that virtual
    // terminal every placement is 1:1 with plain Helix, which is why the pty
    // size is otherwise passed through untouched.
    let surface_rows = crossterm::terminal::size()
        .map(|(_, rows)| rows)
        .unwrap_or(40);
    let terminal_rows = surface_rows.saturating_sub(1).max(1);
    let area = Rect::new(
        0,
        0,
        columns.max(1).clamp(1, u16::MAX as u32) as u16,
        terminal_rows,
    );
    editor.resize(area);
    input.resize(area);
}

fn current_rope(editor: &Editor) -> Rope {
    let view = editor.tree.get(editor.tree.focus);
    editor.document(view.doc).unwrap().text().clone()
}

fn current_cursor(editor: &Editor) -> usize {
    let view = editor.tree.get(editor.tree.focus);
    let doc = editor.document(view.doc).unwrap();
    doc.selection(view.id)
        .primary()
        .cursor(doc.text().slice(..))
}

fn select(editor: &mut Editor, position: usize) {
    let view_id = editor.tree.focus;
    let doc_id = editor.tree.get(view_id).doc;
    let doc = editor.document_mut(doc_id).unwrap();
    let position = position.min(doc.text().len_chars());
    doc.set_selection(view_id, Selection::point(position));
}

fn dispatch_key(key: &tern_sdk::Key, editor: &mut Editor, input: &mut Compositor, jobs: &mut Jobs) {
    if key.name == "paste" {
        dispatch(
            Event::Paste(key.text.clone().unwrap_or_default()),
            editor,
            input,
            jobs,
        );
        return;
    }
    let Some(code) = key_code(key) else {
        return;
    };
    let mut modifiers = KeyModifiers::empty();
    if key.ctrl {
        modifiers |= KeyModifiers::CONTROL;
    }
    if key.alt {
        modifiers |= KeyModifiers::ALT;
    }
    if key.meta {
        modifiers |= KeyModifiers::SUPER;
    }
    if key.shift && !matches!(code, KeyCode::Char(_)) {
        modifiers |= KeyModifiers::SHIFT;
    }
    dispatch(
        Event::Key(KeyEvent { code, modifiers }),
        editor,
        input,
        jobs,
    );
}

fn dispatch(event: Event, editor: &mut Editor, input: &mut Compositor, jobs: &mut Jobs) {
    input.handle_event(
        &event,
        &mut InputContext {
            editor,
            jobs,
            scroll: None,
        },
    );
}

fn key_code(key: &tern_sdk::Key) -> Option<KeyCode> {
    Some(match key.name.as_str() {
        "escape" => KeyCode::Esc,
        "enter" => KeyCode::Enter,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "tab" => KeyCode::Tab,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "page_up" => KeyCode::PageUp,
        "page_down" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        name => {
            if let Some(number) = name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                KeyCode::F(number)
            } else {
                let text = key.text.as_deref().unwrap_or(name);
                let mut chars = text.chars();
                let character = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                KeyCode::Char(character)
            }
        }
    })
}

#[cfg(test)]
#[path = "../tests/impls/tern_frontend.rs"]
mod tests;
