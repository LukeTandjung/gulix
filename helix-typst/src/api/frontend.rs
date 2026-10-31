//! Tern Surface Protocol frontend for inline Typst block previews.

use std::path::Path;

use tern_sdk::{Input, Options, Session, SurfaceOptions};

use crate::{
    DocumentPreview, FragmentId, RenderRuntime, TypstCliRenderer, TypstRenderer, UsvgProcessor,
    DEFAULT_SETTINGS,
};

use super::{active::ActiveFragment, frame::PreviewFrame, session::HelixSession, style::Style};

#[derive(Clone, Debug)]
pub enum Msg {
    Reveal(FragmentId),
    Deactivate,
}

/// Runs Helix inside Tern and coordinates preview rendering.
pub fn run(path: Option<&Path>) -> anyhow::Result<()> {
    let Some(mut session) = Session::<Msg>::connect(Options::new().app("gulix"))? else {
        println!("hxt needs to run inside Tern; use hx for the terminal frontend.");
        return Ok(());
    };

    let mut helix = HelixSession::new(path)?;
    helix.resize(session.caps().cols);
    let source_root = path
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut context = helix.render_context(&session, source_root);
    let mut document = DocumentPreview::new(helix.current_rope(), context.clone())
        .map_err(|error| anyhow::anyhow!("invalid initial fragments: {error:?}"))?;

    let mut renderer = TypstCliRenderer::new(Path::new("typst").to_path_buf());
    renderer
        .load_settings(DEFAULT_SETTINGS)
        .map_err(|error| anyhow::anyhow!("failed to load Typst fonts: {error:?}"))?;
    let mut runtime = RenderRuntime::new(renderer, UsvgProcessor::new());

    let surface = session.open(
        SurfaceOptions::screen()
            .title("Gulix Typst")
            .role("gulix.typst"),
    )?;
    let background_css = format!(
        ".sf-region, .sf-rows, .sf-row, .sf-col, .sf-status {{ background-color: {} !important; }} .sf-rows, .sf-row, .sf-status {{ width: 100% !important; max-width: none !important; margin: 0 !important; }} .sf-rows[data-role='gulix-inline-text'] {{ width: auto !important; }} .sf-col[data-role='gulix-status'] {{ background-color: transparent !important; }} .sf-rows-mark {{ display: none !important; }}",
        Style::background_css(&helix.editor, session.caps().dark)
    );
    session.stylesheet(surface, "gulix", Some(&background_css))?;

    let mut active = ActiveFragment::new();
    let mut synchronize_initial = true;
    let mut revealed_initial = false;
    let mut initial_reveal_pending = false;

    loop {
        let next_context = helix.render_context(&session, context.source_root.clone());
        if next_context != context {
            context = next_context;
        }

        runtime.schedule_missing(&mut document, context.clone());
        runtime.poll(&mut document);
        active.synchronize(&mut document, &helix, !synchronize_initial);
        synchronize_initial = false;

        PreviewFrame::new(
            &mut session,
            surface,
            &mut helix.editor,
            &mut helix.input,
            &mut helix.jobs,
        )?
        .render(&mut document, active.active(), &mut helix.editor)?;

        while let Ok(callback) = helix.jobs.callbacks.try_recv() {
            if let Some(job) =
                helix
                    .jobs
                    .handle_callback(&mut helix.editor, &mut helix.input, Ok(Some(callback)))
            {
                helix.jobs.add(job);
            }
        }

        let Some(event) = session.next(Some(std::time::Duration::from_millis(50)))? else {
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
                active.resume();
                let previous_source = helix.current_rope();
                helix.dispatch_key(&key);

                let source = helix.current_rope();
                if source != previous_source {
                    document
                        .replace_source(source)
                        .map_err(|error| anyhow::anyhow!("invalid edited fragments: {error:?}"))?;
                    if active
                        .active()
                        .is_some_and(|_| document.presentations(active.active()).is_none())
                    {
                        active.deactivate(&mut document);
                    }
                }

                if helix.editor.should_close() {
                    break;
                }
            }
            Input::Msg(Msg::Reveal(id), _) => active.reveal(id, &mut document, &mut helix),
            Input::Msg(Msg::Deactivate, _) => {
                active.resume();
                active.deactivate(&mut document);
            }
            Input::Event(event) => {
                if !revealed_initial && matches!(event, tern_sdk::Event::Resize(_)) {
                    initial_reveal_pending = true;
                }

                let focus_lost = match &event {
                    tern_sdk::Event::Visible(event) => !event.visible,
                    tern_sdk::Event::Resize(event) => event.visible == Some(false),
                    _ => matches!(event.name(), "blur" | "focusout" | "deactivate"),
                };
                if focus_lost {
                    active.suspend(&mut document);
                }

                let focus_gained = match &event {
                    tern_sdk::Event::Visible(event) => event.visible,
                    _ => matches!(event.name(), "focus" | "focusin" | "activate"),
                };
                if focus_gained {
                    active.resume();
                }

                helix.resize(session.caps().cols);
            }
        }
    }

    session.close()?;

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/api/frontend.rs"]
mod tests;
