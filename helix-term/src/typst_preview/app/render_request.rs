//! Request preparation and execution for one fragment render.

use getset::CopyGetters;

use std::sync::Arc;

use crate::typst_preview::{
    core::{FragmentId, FragmentKind, RenderContext},
    ports::{CanvasFit, CompileRequest, RenderedFragment, SvgProcessor, TypstRenderer},
};

/// An owned request and staleness token for one asynchronous fragment render.
#[derive(Clone, CopyGetters, Debug, PartialEq)]
pub struct RenderRequest {
    #[getset(get_copy = "pub")]
    pub id: FragmentId,
    pub kind: FragmentKind,
    pub source: String,
    pub context: Arc<RenderContext>,
    pub canvas_fit: CanvasFit,
}

#[derive(Debug)]
pub enum RenderError<TypstError, SvgError> {
    Typst(TypstError),
    Svg(SvgError),
}

impl RenderRequest {
    /// Executes this request through the app-owned compile and SVG-processing pipeline.
    pub fn render<Typst, Svg>(
        &self,
        typst: &Typst,
        svg: &Svg,
    ) -> Result<RenderedFragment, RenderError<Typst::Error, Svg::Error>>
    where
        Typst: TypstRenderer,
        Svg: SvgProcessor,
    {
        let raw = typst
            .render(&CompileRequest {
                kind: self.kind,
                source: &self.source,
                context: &self.context,
            })
            .map_err(RenderError::Typst)?;
        let document = svg
            .simplify(raw.as_bytes(), &self.context.source_root)
            .map_err(RenderError::Svg)?;
        let document = svg.expand_images(document).map_err(RenderError::Svg)?;

        match self.canvas_fit {
            CanvasFit::AsTypeset => Ok(svg.into_fragment(document)),
            CanvasFit::InkTight => match svg.ink_bounds(&document) {
                Some(ink) => svg
                    .fit_canvas_to_ink(document, ink)
                    .map_err(RenderError::Svg),
                None => Ok(svg.into_fragment(document)),
            },
        }
    }
}
