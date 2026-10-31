use getset::CopyGetters;

use crate::{DocumentPreview, FragmentId, Presentation};

use super::session::HelixSession;

/// Activation state shared by cursor navigation, click handling, and focus changes.
#[derive(CopyGetters)]
pub struct ActiveFragment {
    #[getset(get_copy = "pub")]
    active: Option<FragmentId>,
    preview_suspended: bool,
}

impl ActiveFragment {
    pub fn new() -> Self {
        Self {
            active: None,
            preview_suspended: false,
        }
    }

    pub fn synchronize(
        &mut self,
        document: &mut DocumentPreview,
        session: &HelixSession,
        allow_start: bool,
    ) {
        if self.preview_suspended {
            self.deactivate(document);
            return;
        }
        let cursor = session.current_cursor();
        let navigating = self.active.is_some() || allow_start;

        if let Some(id) = self.active {
            let contains_cursor = document
                .presentations(self.active)
                .and_then(|mut presentations| presentations.find(|(fragment, _)| *fragment == id))
                .is_some_and(|(_, presentation)| {
                    let range = match presentation {
                        Presentation::Source { range, .. }
                        | Presentation::Rendered { range, .. } => range,
                    };
                    cursor >= range.start && cursor <= range.end
                });
            if contains_cursor {
                return;
            }

            drop(document.presentations(None));
            self.active = None;
        }

        let containing = document
            .presentations(self.active)
            .into_iter()
            .flatten()
            .find_map(|(id, presentation)| {
                let range = match presentation {
                    Presentation::Source { range, .. } | Presentation::Rendered { range, .. } => {
                        range
                    }
                };
                let contains = if navigating {
                    cursor >= range.start && cursor < range.end
                } else {
                    cursor > range.start && cursor < range.end
                };
                contains.then_some(id)
            });

        if let Some(id) = containing {
            if document.presentations(Some(id)).is_some() {
                self.active = Some(id);
            }
        }
    }

    pub fn reveal(
        &mut self,
        id: FragmentId,
        document: &mut DocumentPreview,
        session: &mut HelixSession,
    ) {
        self.preview_suspended = false;

        if let Some(presentation) = document
            .presentations(Some(id))
            .and_then(|mut presentations| presentations.find(|(fragment, _)| *fragment == id))
            .map(|(_, presentation)| presentation)
        {
            let start = match presentation {
                Presentation::Source { range, .. } | Presentation::Rendered { range, .. } => {
                    range.start
                }
            };

            session.select(start);
            self.active = Some(id);
        }
    }

    pub fn deactivate(&mut self, document: &mut DocumentPreview) {
        if self.active.take().is_some() {
            drop(document.presentations(None));
        }
    }

    pub fn suspend(&mut self, document: &mut DocumentPreview) {
        self.preview_suspended = true;
        self.deactivate(document);
    }

    pub fn resume(&mut self) {
        self.preview_suspended = false;
    }
}
