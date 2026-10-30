//! Document-level composition for discovery, rendering, acceptance, and presentation.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

use helix_core::{Rope, RopeSlice};

use crate::typst_preview::{
    app::{
        discovery::{discover_fragments, reconcile_fragments},
        render_request::RenderRequest,
    },
    core::{Fragment, FragmentId, FragmentKind, InvalidFragments, PreviewState, RenderContext},
    ports::{CanvasFit, RenderedFragment},
};

/// The current visible representation of one source fragment.
#[derive(Clone, Debug)]
pub enum Presentation<'a> {
    Source {
        range: &'a Range<usize>,
        source: RopeSlice<'a>,
    },
    Rendered {
        range: &'a Range<usize>,
        kind: FragmentKind,
        rendered: &'a RenderedFragment,
    },
}

#[derive(Clone, Debug)]
struct RenderArtifact {
    request: RenderRequest,
    rendered: RenderedFragment,
}

/// Owns one source revision, its current context, and accepted artifacts.
pub struct DocumentPreview {
    source: Rope,
    context: Arc<RenderContext>,
    state: PreviewState,
    artifacts: HashMap<FragmentId, RenderArtifact>,
}

impl DocumentPreview {
    pub fn new(source: Rope, context: RenderContext) -> Result<Self, InvalidFragments> {
        let text = source.to_string();
        let discovered = discover_fragments(&text);
        let fragments = reconcile_fragments(discovered, std::iter::empty::<&Fragment>(), "");
        Ok(Self {
            source,
            context: Arc::new(context),
            state: PreviewState::new(fragments)?,
            artifacts: HashMap::new(),
        })
    }

    /// Reconciles a source revision and revalidates retained artifacts.
    pub fn replace_source(&mut self, source: Rope) -> Result<(), InvalidFragments> {
        let previous_source = self.source.to_string();
        let discovered = discover_fragments(&source.to_string());
        let fragments = reconcile_fragments(discovered, self.state.fragments(), &previous_source);
        self.state.replace_fragments(fragments)?;
        let surviving: HashSet<FragmentId> =
            self.state.fragments().map(|fragment| fragment.id).collect();
        self.artifacts.retain(|id, _| surviving.contains(id));
        self.source = source;
        for id in surviving {
            let Some(artifact) = self.artifacts.get(&id) else {
                continue;
            };
            let current = self
                .state
                .fragments()
                .find(|fragment| fragment.id == id)
                .map(|fragment| {
                    let source = self
                        .source
                        .slice(fragment.range.start..fragment.range.end)
                        .to_string();
                    let canvas_fit = if fragment.kind == FragmentKind::InlineMath {
                        CanvasFit::InkTight
                    } else {
                        CanvasFit::AsTypeset
                    };
                    RenderRequest {
                        id,
                        kind: fragment.kind,
                        source,
                        context: self.context.clone(),
                        canvas_fit,
                    }
                });
            if current.is_some_and(|request| request == artifact.request) {
                self.state.record_render(id);
            }
        }
        Ok(())
    }

    /// Updates context and returns requests that need rendering.
    pub fn requests(&mut self, context: RenderContext) -> Vec<RenderRequest> {
        if self.context.as_ref() != &context {
            self.context = Arc::new(context);
            self.state.invalidate_renders();
        }
        let ids: Vec<FragmentId> = self.state.fragments().map(|fragment| fragment.id).collect();
        let mut requests = Vec::new();
        for id in ids {
            if !self.state.needs_render(id) {
                continue;
            }
            let Some(request) = self
                .state
                .fragments()
                .find(|fragment| fragment.id == id)
                .map(|fragment| {
                    let source = self
                        .source
                        .slice(fragment.range.start..fragment.range.end)
                        .to_string();
                    let canvas_fit = if fragment.kind == FragmentKind::InlineMath {
                        CanvasFit::InkTight
                    } else {
                        CanvasFit::AsTypeset
                    };
                    RenderRequest {
                        id,
                        kind: fragment.kind,
                        source,
                        context: self.context.clone(),
                        canvas_fit,
                    }
                })
            else {
                continue;
            };
            if self
                .artifacts
                .get(&id)
                .is_some_and(|artifact| artifact.request == request)
            {
                self.state.record_render(id);
            } else {
                requests.push(request);
            }
        }
        requests
    }

    /// Publishes a response only when its complete request still matches current state.
    pub fn accept_response(&mut self, request: RenderRequest, rendered: RenderedFragment) -> bool {
        let Some(current) = self
            .state
            .fragments()
            .find(|fragment| fragment.id == request.id)
            .map(|fragment| {
                let source = self
                    .source
                    .slice(fragment.range.start..fragment.range.end)
                    .to_string();
                let canvas_fit = if fragment.kind == FragmentKind::InlineMath {
                    CanvasFit::InkTight
                } else {
                    CanvasFit::AsTypeset
                };
                RenderRequest {
                    id: request.id,
                    kind: fragment.kind,
                    source,
                    context: self.context.clone(),
                    canvas_fit,
                }
            })
        else {
            return false;
        };
        if current != request {
            return false;
        }
        let accepted = self.state.record_render(request.id);
        if accepted {
            self.artifacts
                .insert(request.id, RenderArtifact { request, rendered });
        }
        accepted
    }

    /// Sets the active fragment and returns all presentations. `None` clears activation.
    pub fn presentations(
        &mut self,
        active: Option<FragmentId>,
    ) -> Option<impl Iterator<Item = (FragmentId, Presentation<'_>)> + '_> {
        match active {
            Some(id) => {
                if !self.state.activate(id) {
                    return None;
                }
            }
            None => self.state.deactivate(),
        }

        Some(self.state.fragments().map(|fragment| {
            let id = fragment.id;
            let range = &fragment.range;
            let presentation = match self.state.is_rendered(id) {
                Some(false) => Presentation::Source {
                    source: self.source.slice(range.clone()),
                    range,
                },
                Some(true) => {
                    let rendered = &self
                        .artifacts
                        .get(&id)
                        .expect("a current render must have an accepted artifact")
                        .rendered;
                    Presentation::Rendered {
                        range,
                        kind: fragment.kind,
                        rendered,
                    }
                }
                None => panic!("every fragment must have a presentation"),
            };
            (id, presentation)
        }))
    }
}
