use vstd::prelude::*;

verus! {

#[cfg(verus_only)]
#[verifier::external_type_specification]
#[verifier::external_body]
pub struct ExPathBuf(pub std::path::PathBuf);

/// Inputs shared by every construct; app functions specialise source, not this context.
#[derive(Clone, Debug, PartialEq)]
#[verifier::allow(autoderive_clone_without_spec)]
pub struct RenderContext {
    /// Relative to surrounding editor text; Typst's 11pt default is not imposed here.
    pub body_font_size_px: f32,
    pub foreground_rgba: [f32; 4],
    /// Available text-column measure, not an instruction to stretch intrinsic content.
    pub available_width_px: f32,
    /// Resolves only basic local images and figures within the document's trust boundary.
    pub source_root: std::path::PathBuf,
}

} // verus!
