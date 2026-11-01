use std::{path::Path, sync::Arc};

use arc_swap::{access::Map, ArcSwap};
use helix_core::{syntax, Rope, Selection};
use helix_loader::workspace_trust::WorkspaceTrust;
use helix_view::{
    editor::{Action, Editor},
    graphics::Rect,
    input::{Event, KeyCode, KeyEvent, KeyModifiers},
    theme,
};
use tern_sdk::{Key, Session};

use helix_term::{
    compositor::{Compositor, Context as InputContext},
    config::{Config, ConfigLoadError},
    job::Jobs,
    keymap::Keymaps,
    ui::EditorView,
};

use crate::{RenderContext, TerminalLayout};

use super::{frontend::Msg, style::Style};

/// Helix state shared by session setup, input dispatch, and frame rendering.
pub struct HelixSession {
    pub editor: Editor,
    pub input: Compositor,
    pub jobs: Jobs,
}

impl HelixSession {
    pub fn new(path: Option<&Path>) -> anyhow::Result<Self> {
        helix_loader::initialize_config_file(None);

        let loaded = match Config::load_default() {
            Ok(config) => config,
            Err(ConfigLoadError::Error(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Config::default()
            }
            Err(error) => anyhow::bail!("Failed to load Helix configuration: {error}"),
        };

        let trust = WorkspaceTrust::new((&loaded.editor.workspace_trust).into());
        let config = Arc::new(ArcSwap::from_pointee(loaded));
        let handlers = helix_term::handlers::setup(config.clone());

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

        Ok(Self {
            editor,
            input,
            jobs: Jobs::new(),
        })
    }

    pub fn resize(&mut self, columns: u32) {
        let surface_rows = crossterm::terminal::size()
            .map(|(_, rows)| rows)
            .unwrap_or(40);
        let terminal_rows = TerminalLayout::terminal_rows(surface_rows);
        let area = Rect::new(
            0,
            0,
            columns.max(1).clamp(1, u16::MAX as u32) as u16,
            terminal_rows,
        );

        self.editor.resize(area);
        self.input.resize(area);
    }

    pub fn render_context(
        &self,
        session: &Session<Msg>,
        source_root: std::path::PathBuf,
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
            foreground_rgba: self
                .editor
                .theme
                .get("ui.text")
                .fg
                .and_then(Style::color_rgba)
                .unwrap_or(fallback),
            available_width_px: caps.cols.saturating_sub(8).max(1) as f32 * cell_width,
            source_root,
        }
    }

    pub fn current_rope(&self) -> Rope {
        let view = self.editor.tree.get(self.editor.tree.focus);
        self.editor.document(view.doc).unwrap().text().clone()
    }

    pub fn current_cursor(&self) -> usize {
        let view = self.editor.tree.get(self.editor.tree.focus);
        let doc = self.editor.document(view.doc).unwrap();
        doc.selection(view.id)
            .primary()
            .cursor(doc.text().slice(..))
    }

    pub fn select(&mut self, position: usize) {
        let view_id = self.editor.tree.focus;
        let doc_id = self.editor.tree.get(view_id).doc;
        let doc = self.editor.document_mut(doc_id).unwrap();
        let position = position.min(doc.text().len_chars());

        doc.set_selection(view_id, Selection::point(position));
    }

    pub fn dispatch_key(&mut self, key: &Key) {
        if key.name == "paste" {
            self.dispatch(Event::Paste(key.text.clone().unwrap_or_default()));
            return;
        }

        let Some(code) = Self::key_code(key) else {
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

        self.dispatch(Event::Key(KeyEvent { code, modifiers }));
    }

    pub fn key_code(key: &Key) -> Option<KeyCode> {
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

    fn dispatch(&mut self, event: Event) {
        self.input.handle_event(
            &event,
            &mut InputContext {
                editor: &mut self.editor,
                jobs: &mut self.jobs,
                scroll: None,
            },
        );
    }
}
