//! Public asynchronous composition for inline previews.
//!
//! Worker lifecycle belongs here; document state and presentation remain app-owned.

use std::collections::{HashMap, HashSet};

use tokio::task::JoinSet;

use crate::{
    adapters::{SvgError, TypstCliError, TypstCliRenderer, UsvgProcessor},
    app::{DocumentPreview, RenderError, RenderRequest},
    core::{FragmentId, RenderContext},
    ports::RenderedFragment,
};

type Completion = (
    RenderRequest,
    Result<RenderedFragment, RenderError<TypstCliError, SvgError>>,
);

/// Composes concrete renderers and blocking workers. The caller owns the document.
pub struct RenderRuntime {
    renderer: TypstCliRenderer,
    svg: UsvgProcessor,
    tasks: JoinSet<Completion>,
    pending: HashSet<FragmentId>,
    failed: HashMap<FragmentId, RenderRequest>,
}

impl RenderRuntime {
    pub fn new(renderer: TypstCliRenderer, svg: UsvgProcessor) -> Self {
        Self {
            renderer,
            svg,
            tasks: JoinSet::new(),
            pending: HashSet::new(),
            failed: HashMap::new(),
        }
    }

    /// Spawns at most one outstanding task per fragment outside Tern's render path.
    pub fn schedule_missing(&mut self, document: &mut DocumentPreview, context: RenderContext) {
        for request in document.requests(context) {
            let id = request.id();
            if self.pending.contains(&id)
                || self
                    .failed
                    .get(&id)
                    .is_some_and(|failed| failed == &request)
            {
                continue;
            }
            self.pending.insert(id);
            let renderer = self.renderer.clone();
            let svg = self.svg;
            self.tasks.spawn_blocking(move || {
                let result = request.render(&renderer, &svg);
                (request, result)
            });
        }
    }

    /// Applies completed requests and returns whether any visible artifact changed.
    pub fn poll(&mut self, document: &mut DocumentPreview) -> bool {
        let mut changed = false;
        while let Some(completed) = self.tasks.try_join_next() {
            let Ok((request, result)) = completed else {
                // A cancelled task cannot leave its fragment permanently suppressed.
                self.pending.clear();
                continue;
            };
            let id = request.id();
            self.pending.remove(&id);
            match result {
                Ok(rendered) => {
                    if document.accept_response(request, rendered) {
                        self.failed.remove(&id);
                        changed = true;
                    }
                }
                Err(_) => {
                    // Suppress retries only for this exact failed request.
                    // Unrelated edits leave other fragments' failures alone.
                    self.failed.insert(id, request.clone());
                }
            }
        }
        changed
    }
}
