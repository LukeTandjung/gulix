//! Application-owned Typst compilation capability. No CLI or process types leak here.

use vstd::prelude::*;

use crate::typst_preview::core::{FragmentKind, RenderContext};

verus! {

/// Typesetting resources and rules retained by a renderer after setup.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypstSettings<'a> {
    pub fonts: &'a [(&'a str, &'a [u8])],
    pub math_font: &'a str,
    pub body_font: &'a str,
    pub math_font_scale: f32,
    pub construct_rules: bool,
    pub inline_margin_pt: f32,
    pub block_margin_pt: f32,
}

/// The subset of a render request consumed by the Typst compiler.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompileRequest<'a> {
    pub kind: FragmentKind,
    pub source: &'a str,
    pub context: &'a RenderContext,
}

/// The one Typst compilation port.
pub trait TypstRenderer: Sync {
    type Error;

    /// Loads typesetting resources and rules once for later renders.
    fn load_settings(&mut self, settings: TypstSettings) -> Result<(), Self::Error>;

    fn render(&self, request: &CompileRequest) -> (result: Result<String, Self::Error>)
        ensures
            result is Ok ==> result->Ok_0@.len() > 0;
}

} // verus!
