use std::path::PathBuf;

use helix_core::Rope;
use helix_term::typst_preview::{DocumentPreview, FragmentId, RenderContext, RenderedFragment};

pub fn document(source: &str) -> DocumentPreview {
    DocumentPreview::new(Rope::from(source), context()).expect("test fragments must be valid")
}

pub fn context() -> RenderContext {
    RenderContext {
        body_font_size_px: 16.0,
        foreground_rgba: [0.25, 0.5, 0.75, 1.0],
        available_width_px: 640.0,
        source_root: PathBuf::from("."),
    }
}

pub fn rendered() -> RenderedFragment {
    RenderedFragment {
        svg: "<svg xmlns=\"http://www.w3.org/2000/svg\"/>".into(),
        intrinsic_width_px: 20.0,
        intrinsic_height_px: 10.0,
    }
}

pub fn ids(document: &mut DocumentPreview) -> Vec<FragmentId> {
    document
        .presentations(None)
        .expect("presentation")
        .map(|(id, _)| id)
        .collect()
}
