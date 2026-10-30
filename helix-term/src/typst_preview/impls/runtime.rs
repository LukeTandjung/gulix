//! Production asynchronous runtime for inline previews.

use std::collections::{HashMap, HashSet};

use helix_core::Rope;
use tokio::task::JoinSet;

use crate::typst_preview::{
    adapters::{SvgError, TypstCliError, TypstCliRenderer, UsvgProcessor},
    app::{DocumentPreview, Presentation, RenderError, RenderRequest},
    core::{FragmentId, InvalidFragments, RenderContext},
    ports::RenderedFragment,
};

type Completion = (
    RenderRequest,
    Result<RenderedFragment, RenderError<TypstCliError, SvgError>>,
);

/// Owns renderer tasks; dropping the runtime detaches outstanding blocking work.
pub struct RenderRuntime {
    document: DocumentPreview,
    renderer: TypstCliRenderer,
    svg: UsvgProcessor,
    tasks: JoinSet<Completion>,
    pending: HashSet<FragmentId>,
    failed: HashMap<FragmentId, RenderRequest>,
}

impl RenderRuntime {
    pub fn new(document: DocumentPreview, renderer: TypstCliRenderer, svg: UsvgProcessor) -> Self {
        Self {
            document,
            renderer,
            svg,
            tasks: JoinSet::new(),
            pending: HashSet::new(),
            failed: HashMap::new(),
        }
    }

    pub fn replace_source(&mut self, source: Rope) -> Result<(), InvalidFragments> {
        self.document.replace_source(source)
    }

    pub fn activate(&mut self, id: FragmentId) -> Option<Presentation<'_>> {
        self.document
            .presentations(Some(id))?
            .find(|(fragment, _)| *fragment == id)
            .map(|(_, presentation)| presentation)
    }

    pub fn deactivate(&mut self) {
        drop(self.document.presentations(None));
    }

    pub fn presentations(
        &mut self,
        active: Option<FragmentId>,
    ) -> Option<impl Iterator<Item = (FragmentId, Presentation<'_>)> + '_> {
        self.document.presentations(active)
    }

    /// Spawns at most one outstanding task per fragment outside Tern's render path.
    pub fn schedule_missing(&mut self, context: RenderContext) {
        for request in self.document.requests(context) {
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
    pub fn poll(&mut self) -> bool {
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
                    if self.document.accept_response(request, rendered) {
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
