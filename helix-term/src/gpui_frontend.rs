//! Native source-editor spike. GPUI owns the application and its mutable editor.
//! The compositor is used for Helix input callbacks only, never for painting.

mod geometry;
use geometry::{FONT_FAMILY, FONT_SIZE, INSET_X, INSET_Y, LINE_HEIGHT};
use std::{path::Path, sync::Arc, task::Poll};

use arc_swap::{access::Map, ArcSwap};
use futures_util::{FutureExt, StreamExt};
#[cfg(test)]
use gpui::rgb;
use gpui::{
    div, prelude::*, px, uniform_list, Context, FocusHandle, HighlightStyle, KeyDownEvent,
    ScrollStrategy, StyledText, UniformListScrollHandle, Window,
};
use helix_core::syntax;
use helix_loader::workspace_trust::WorkspaceTrust;
use helix_view::{
    editor::{Action, EditorEvent},
    graphics::Rect,
    input::{Event, KeyCode, KeyEvent, KeyModifiers},
    theme, Editor,
};

use crate::{
    compositor::{Compositor, Context as InputContext},
    config::Config,
    job::Jobs,
    keymap::Keymaps,
    ui,
};

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
    create_session_with_config(path, loaded)
}

fn create_session_with_config(
    path: Option<&Path>,
    loaded: Config,
) -> anyhow::Result<(Editor, Compositor, Jobs)> {
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
    let mut languages = helix_core::config::user_lang_config(&trust)?;
    // Load syntax and indentation, but do not start unported language services.
    languages.language_server.clear();
    for language in &mut languages.language {
        language.language_servers.clear();
    }
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
    input.push(Box::new(ui::EditorView::new(Keymaps::new(Box::new(
        Map::new(config, |config: &Config| &config.keys),
    )))));
    Ok((editor, input, Jobs::new()))
}

pub struct Workspace {
    editor: Editor,
    input: Compositor,
    jobs: Jobs,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    scroll_item_count: usize,
    visual_rows: Arc<Vec<geometry::VisualRow>>,
    service_waker: Option<std::task::Waker>,
}

impl Workspace {
    pub fn new(
        path: Option<&Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<Self> {
        let (editor, input, jobs) = create_session(path)?;
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let workspace = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            workspace
                .update(cx, |state, cx| {
                    if state.editor.documents().any(|doc| doc.is_modified())
                        || state.editor.write_count > 0
                        || !state.jobs.wait_futures.is_empty()
                    {
                        state.editor.set_error(
                            "Unsaved changes or pending writes. Use :w, or :q! to discard.",
                        );
                        cx.notify();
                        false
                    } else {
                        true
                    }
                })
                .unwrap_or(true)
        });
        let state = Self {
            editor,
            input,
            jobs,
            focus,
            scroll: UniformListScrollHandle::new(),
            scroll_item_count: 0,
            visual_rows: Arc::new(Vec::new()),
            service_waker: None,
        };
        // GPUI awaits service readiness. There is no timer-driven polling loop
        // and no terminal Application. Each poll borrows the entity only briefly.
        cx.spawn(async move |this, cx| loop {
            let alive = futures_util::future::poll_fn(|task| {
                this.update(cx, |state, cx| state.poll_services(task, cx))
                    .unwrap_or(Poll::Ready(false))
            })
            .await;
            if !alive {
                break;
            }
        })
        .detach();
        Ok(state)
    }

    fn dispatch(&mut self, event: Event) {
        self.input.handle_event(
            &event,
            &mut InputContext {
                editor: &mut self.editor,
                jobs: &mut self.jobs,
                scroll: None,
            },
        );
        if let Some(waker) = &self.service_waker {
            waker.wake_by_ref();
        }
    }

    fn poll_services(
        &mut self,
        task: &mut std::task::Context<'_>,
        cx: &mut Context<Self>,
    ) -> Poll<bool> {
        self.service_waker = Some(task.waker().clone());
        if let Poll::Ready(Some(callback)) = self.jobs.callbacks.poll_recv(task) {
            if let Some(job) =
                self.jobs
                    .handle_callback(&mut self.editor, &mut self.input, Ok(Some(callback)))
            {
                self.jobs.add(job);
            }
            cx.notify();
            return Poll::Ready(true);
        }
        if let Poll::Ready(Some(result)) = self.jobs.wait_futures.poll_next_unpin(task) {
            if let Some(job) = self
                .jobs
                .handle_callback(&mut self.editor, &mut self.input, result)
            {
                self.jobs.add(job);
            }
            cx.notify();
            return Poll::Ready(true);
        }
        if let Poll::Ready(Some(message)) = self.jobs.status_messages.poll_recv(task) {
            self.editor.set_status(message.message);
            cx.notify();
            return Poll::Ready(true);
        }
        let event = self.editor.wait_event().boxed_local().poll_unpin(task);
        if let Poll::Ready(event) = event {
            match event {
                EditorEvent::DocumentSaved(Ok(saved)) => {
                    if let Some(doc) = self.editor.document_mut(saved.doc_id) {
                        doc.set_last_saved_revision(saved.revision, saved.save_time);
                    }
                    self.editor.set_status("Saved");
                }
                EditorEvent::DocumentSaved(Err(error)) => self.editor.set_error(error.to_string()),
                EditorEvent::IdleTimer => {
                    self.editor.clear_idle_timer();
                    self.dispatch(Event::IdleTimeout);
                }
                EditorEvent::Redraw => self.editor.needs_redraw = false,
                // set_theme already updated the editor and syntax scopes. Repaint below.
                EditorEvent::ConfigEvent(helix_view::editor::ConfigEvent::ThemeChanged) => {}
                _ => self
                    .editor
                    .set_error("This service event is not yet supported by the GUI spike"),
            }
            cx.notify();
            return Poll::Ready(true);
        }
        Poll::Pending
    }

    /// Geometry observations used by the opt-in native-window test.
    #[cfg(feature = "gpui-window-test")]
    pub fn viewport_metrics(&self) -> (Rect, usize, f32, usize) {
        let view = self.editor.tree.get(self.editor.tree.focus);
        let doc = self.editor.document(view.doc).unwrap();
        (
            view.inner_area(doc),
            geometry::row_at_char(&self.visual_rows, doc.view_offset(view.id).anchor)
                + doc.view_offset(view.id).vertical_offset,
            -f32::from(self.scroll.0.borrow().base_handle.offset().y) / self.row_height(),
            doc.view_offset(view.id).horizontal_offset,
        )
    }

    #[cfg(feature = "gpui-window-test")]
    pub fn wrapping_metrics(
        &self,
    ) -> (
        usize,
        Option<helix_core::Position>,
        Option<helix_core::Position>,
    ) {
        use helix_core::unicode::width::UnicodeWidthStr;
        let view = self.editor.tree.get(self.editor.tree.focus);
        let doc = self.editor.document(view.doc).unwrap();
        let position = doc
            .selection(view.id)
            .primary()
            .cursor(doc.text().slice(..));
        let index = geometry::row_at_char(&self.visual_rows, position);
        let top = self.viewport_metrics().2.round() as usize;
        let actual = self.visual_rows.get(index).and_then(|row| {
            let bytes = row.display.display_range(position..position + 1)?;
            let row_index = index.checked_sub(top)?;
            (row_index < view.inner_area(doc).height as usize).then(|| {
                helix_core::Position::new(row_index, row.display.text[..bytes.start].width())
            })
        });
        (
            self.visual_rows
                .iter()
                .filter(|row| row.line == 0 && !row.first)
                .count(),
            actual,
            view.screen_coords_at_pos(doc, doc.text().slice(..), position),
        )
    }
    fn row_height(&self) -> f32 {
        // GPUI records the whole list's content size, not a single row's size.
        // Use the count from that layout, before the next render changes it.
        self.scroll
            .0
            .borrow()
            .last_item_size
            .filter(|_| self.scroll_item_count > 0)
            .map(|size| f32::from(size.contents.height) / self.scroll_item_count as f32)
            .filter(|height| *height > 0.)
            .unwrap_or_else(|| LINE_HEIGHT.round())
    }
    // Commit native wheel movement to the focused Helix view before commands run.
    fn read_native_scroll(&mut self) {
        let top = (-f32::from(self.scroll.0.borrow().base_handle.offset().y) / self.row_height())
            .floor()
            .max(0.) as usize;
        let view = self.editor.tree.get(self.editor.tree.focus);
        let (view_id, doc_id) = (view.id, view.doc);
        let doc = self.editor.document_mut(doc_id).unwrap();
        let mut offset = doc.view_offset(view_id);
        let top = top.min(self.visual_rows.len().saturating_sub(1));
        if !self.visual_rows.is_empty() {
            let anchor_row = self.visual_rows[..=top]
                .iter()
                .rposition(|row| !row.source.is_empty())
                .unwrap_or(0);
            offset.anchor = self.visual_rows[anchor_row]
                .source
                .start
                .min(doc.text().len_chars());
            offset.vertical_offset = top - anchor_row;
        }
        doc.set_view_offset(view_id, offset);
    }

    fn source_height(&self, window: &Window) -> f32 {
        let status_height = if self.editor.status_msg.is_some() {
            LINE_HEIGHT
        } else {
            0.
        };
        (f32::from(window.viewport_size().height) - 32. - LINE_HEIGHT - INSET_Y - status_height)
            .max(1.)
    }

    fn resize_viewport(&mut self, window: &Window, cx: &Context<Self>) -> f32 {
        let font = cx.text_system().resolve_font(&gpui::font(FONT_FAMILY));
        let cell = cx
            .text_system()
            .advance(font, px(FONT_SIZE), 'm')
            .map(|size| f32::from(size.width))
            .unwrap_or(8.)
            .max(1.);
        let size = window.viewport_size();
        let view = self.editor.tree.get(self.editor.tree.focus);
        let doc = self.editor.document(view.doc).unwrap();
        let columns = ((f32::from(size.width) - INSET_X) / cell).floor().max(1.) as u16;
        let rows = (self.source_height(window) / self.row_height())
            .floor()
            .max(1.) as u16;
        let area = Rect::new(
            0,
            0,
            columns.max(view.gutter_offset(doc).saturating_add(1)),
            rows.saturating_add(1),
        );
        self.editor.resize(area);
        self.input.resize(area);
        cell
    }

    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = translate_key(event) {
            self.dispatch(Event::Key(key));
            if self.editor.should_close() {
                cx.quit();
                return;
            }
            cx.notify();
        }
    }
}

mod cursor;
mod highlights;
mod styling;
impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cell_width = self.resize_viewport(window, cx);
        let base = styling::base_style(&self.editor);
        let colors = styling::highlight(base);
        let workspace = cx.entity();
        let view = self
            .editor
            .tree
            .views()
            .find(|(_, focused)| *focused)
            .map(|(view, _)| view);
        let title = view
            .and_then(|view| self.editor.document(view.doc).unwrap().path())
            .map(|path| format!("{} - Helix", path.display()))
            .unwrap_or_default();
        let (text, statusline, horizontal_offset) = match view {
            Some(view) => {
                let doc = self.editor.document(view.doc).unwrap();
                let format = doc.text_format(view.inner_area(doc).width, Some(&self.editor.theme));
                let annotations = view.text_annotations(doc, Some(&self.editor.theme));
                self.visual_rows = Arc::new(geometry::visual_rows(
                    doc.text().slice(..),
                    &format,
                    &annotations,
                ));
                let offset = doc.view_offset(view.id);
                self.scroll.scroll_to_item_strict(
                    geometry::row_at_char(&self.visual_rows, offset.anchor)
                        + offset.vertical_offset,
                    ScrollStrategy::Top,
                );
                (
                    doc.text().clone(),
                    styling::statusline(&self.editor, doc, view),
                    if format.soft_wrap {
                        0
                    } else {
                        offset.horizontal_offset
                    },
                )
            }
            None => (helix_core::Rope::new(), div(), 0),
        };
        let visual_rows = self.visual_rows.clone();
        let status = self
            .editor
            .status_msg
            .as_ref()
            .map(|(message, _)| message.to_string())
            .unwrap_or_default();
        let prompt = self
            .input
            .find::<ui::Prompt>()
            .map(|prompt| styling::prompt::render(&self.editor, prompt, window));
        // Helix permits positioning the last source line at the top of the view.
        let trailing_rows = (self.source_height(window) / self.row_height())
            .ceil()
            .max(1.) as usize;
        let item_count = visual_rows.len().saturating_add(trailing_rows);
        self.scroll_item_count = item_count;
        div()
            .relative()
            .on_scroll_wheel(cx.listener(|_, _, window, cx| {
                cx.defer_in(window, |state, _, cx| {
                    state.read_native_scroll();
                    cx.notify();
                });
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background_color.unwrap())
            .text_color(colors.color.unwrap())
            .font_family(FONT_FAMILY)
            .text_size(px(FONT_SIZE))
            .line_height(px(LINE_HEIGHT))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .child(
                div()
                    .h(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .items_center()
                    .text_size(px(12.))
                    .child(title),
            )
            .child(
                uniform_list("source", item_count, move |range, window, cx| {
                    let state = workspace.read(cx);
                    let view = state.editor.tree.get(state.editor.tree.focus);
                    let doc = state.editor.document(view.doc).unwrap();
                    let first = visual_rows
                        .get(range.start)
                        .map_or(text.len_chars(), |row| row.source.start);
                    let end = visual_rows
                        .get(range.end.saturating_sub(1))
                        .map_or(text.len_chars() + 1, |row| row.source.end);
                    let styles = highlights::visible_styles(
                        &state.editor,
                        doc,
                        view,
                        first..end,
                        true,
                        window.is_window_active(),
                    );
                    range
                        .map(|row_index| {
                            let Some(row) = visual_rows.get(row_index) else {
                                return div().h(px(LINE_HEIGHT));
                            };
                            let projected = &row.display;
                            let mut highlights = Vec::new();
                            for (range, style) in &styles {
                                if let Some(bytes) = projected.display_range(range.clone()) {
                                    highlights
                                        .push((bytes, styling::highlight(base.patch(*style))));
                                }
                            }
                            let caret = cursor::render(
                                &state.editor,
                                doc,
                                view,
                                projected,
                                0,
                                horizontal_offset,
                                cell_width,
                            );
                            div()
                                .h(px(LINE_HEIGHT))
                                .flex()
                                .child(if row.first {
                                    styling::gutter(&state.editor, doc, view, row.line, cell_width)
                                } else {
                                    div()
                                        .w(px(view.gutter_offset(doc) as f32 * cell_width))
                                        .flex_shrink_0()
                                })
                                .child(
                                    div()
                                        .relative()
                                        .flex_1()
                                        .overflow_hidden()
                                        .child(
                                            div()
                                                .whitespace_nowrap()
                                                .ml(px(-(horizontal_offset as f32) * cell_width))
                                                .child(
                                                    StyledText::new(projected.text.clone())
                                                        .with_highlights(disjoint_highlights(
                                                            highlights,
                                                        )),
                                                ),
                                        )
                                        .children(caret),
                                )
                        })
                        .collect()
                })
                .flex_1()
                .min_h_0()
                .mt(px(INSET_Y))
                .ml(px(INSET_X))
                .track_scroll(&self.scroll),
            )
            .child(statusline)
            .when(!status.is_empty(), |root| {
                root.child(div().h(px(LINE_HEIGHT)).flex_shrink_0().child(status))
            })
            .children(prompt)
    }
}

// StyledText requires ordered, non-overlapping byte ranges. Later highlights
// take priority, so the cursor overrides the selection beneath it.
fn disjoint_highlights(
    highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let mut edges: Vec<_> = highlights
        .iter()
        .flat_map(|(range, _)| [range.start, range.end])
        .collect();
    edges.sort_unstable();
    edges.dedup();
    edges
        .windows(2)
        .filter_map(|edge| {
            highlights
                .iter()
                .rev()
                .find(|(range, _)| range.start <= edge[0] && edge[1] <= range.end)
                .map(|(_, style)| (edge[0]..edge[1], *style))
        })
        .collect()
}

fn translate_key(event: &KeyDownEvent) -> Option<KeyEvent> {
    let key = &event.keystroke;
    let code = match key.key.as_str() {
        "escape" => KeyCode::Esc,
        "enter" => KeyCode::Enter,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "tab" => KeyCode::Tab,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        _ => {
            let text = key.key_char.as_deref().unwrap_or(&key.key);
            let mut chars = text.chars();
            let ch = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            KeyCode::Char(ch)
        }
    };
    let mut modifiers = KeyModifiers::empty();
    if key.modifiers.control {
        modifiers |= KeyModifiers::CONTROL;
    }
    if key.modifiers.alt {
        modifiers |= KeyModifiers::ALT;
    }
    if key.modifiers.shift && !matches!(code, KeyCode::Char(_)) {
        modifiers |= KeyModifiers::SHIFT;
    }
    Some(KeyEvent { code, modifiers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_splits_a_selection_into_disjoint_text_runs() {
        let selection = HighlightStyle {
            background_color: Some(rgb(0x364863).into()),
            ..Default::default()
        };
        let cursor = HighlightStyle {
            background_color: Some(rgb(0xd8dee9).into()),
            ..Default::default()
        };
        let runs = disjoint_highlights(vec![(0..8, selection), (2..4, cursor)]);
        assert_eq!(
            runs.iter()
                .map(|(range, _)| range.clone())
                .collect::<Vec<_>>(),
            vec![0..2, 2..4, 4..8]
        );
        assert_eq!(runs[1].1.background_color, cursor.background_color);
        assert_eq!(runs[2].1.background_color, selection.background_color);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn helix_input_edits_undoes_and_saves() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.txt");
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let (mut editor, mut input, mut jobs) = create_session(Some(&path)).unwrap();
        let press =
            |keys: &[&str], editor: &mut Editor, input: &mut Compositor, jobs: &mut Jobs| {
                for key in keys {
                    input.handle_event(
                        &Event::Key(key.parse().unwrap()),
                        &mut InputContext {
                            editor,
                            jobs,
                            scroll: None,
                        },
                    );
                }
            };
        let text = |editor: &Editor| editor.documents().next().unwrap().text().to_string();
        press(&["i", "λ", "esc"], &mut editor, &mut input, &mut jobs);
        assert_eq!(text(&editor), "λone\ntwo\n");
        press(&["u"], &mut editor, &mut input, &mut jobs);
        assert_eq!(text(&editor), "one\ntwo\n");
        press(&["U"], &mut editor, &mut input, &mut jobs);
        assert_eq!(text(&editor), "λone\ntwo\n");
        press(&["j"], &mut editor, &mut input, &mut jobs);
        let (view, _) = editor.tree.views().find(|(_, focused)| *focused).unwrap();
        let doc = editor.document(view.doc).unwrap();
        assert_eq!(
            doc.text().char_to_line(
                doc.selection(view.id)
                    .primary()
                    .cursor(doc.text().slice(..))
            ),
            1
        );
        press(&[":", "w", "ret"], &mut editor, &mut input, &mut jobs);
        // :w schedules pre-save work before it enters Editor's write queue.
        assert!(!jobs.wait_futures.is_empty());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            jobs.finish(&mut editor, Some(&mut input)).await.unwrap();
            editor.flush_writes().await.unwrap();
        })
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), "λone\ntwo\n");
    }
}
