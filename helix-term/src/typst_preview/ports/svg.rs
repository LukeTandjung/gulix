//! Application-owned SVG operations. No XML or SVG-library types leak here.

use std::path::PathBuf;

use vstd::prelude::*;

verus! {

/// Canvas policy the app picks per construct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasFit {
    /// Keep Typst's canvas untouched, page margins included.
    AsTypeset,
    /// Fit the canvas around visible ink without clipping or shifting its lowest point.
    InkTight,
}

/// Stroke-inclusive visible drawing bounds in canvas coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// Vector output remains independent of Tern blob/session APIs.
#[derive(Clone, Debug, PartialEq)]
#[verifier::allow(autoderive_clone_without_spec)]
pub struct RenderedFragment {
    pub svg: String,
    pub intrinsic_width_px: f32,
    pub intrinsic_height_px: f32,
}

/// SVG operations coordinated by the app-level rendering specialisation.
pub trait SvgProcessor: Sync {
    type Document;
    type Error;

    fn simplify(
        &self,
        svg: &[u8],
        source_root: &PathBuf,
    ) -> Result<Self::Document, Self::Error>;

    fn expand_images(&self, document: Self::Document) -> Result<Self::Document, Self::Error>;

    fn ink_bounds(&self, document: &Self::Document) -> Option<InkBounds>;

    fn into_fragment(&self, document: Self::Document) -> (result: RenderedFragment)
        ensures
            Self::has_valid_dimensions(&result);

    fn fit_canvas_to_ink(
        &self,
        document: Self::Document,
        ink: InkBounds,
    ) -> (result: Result<RenderedFragment, Self::Error>)
        ensures
            result is Ok ==> Self::has_valid_dimensions(&result->Ok_0);

    /// Positive, finite IEEE-754 values, excluding both zero signs, infinities, and NaNs.
    open spec fn positive_finite_f32(value: f32) -> bool {
        vstd::float::FloatBitsProperties::is_finite_spec(&value)
            && !vstd::float::FloatBitsProperties::is_sign_negative_spec(&value)
            && vstd::float::FloatBitsProperties::to_bits_abs_spec(&value) != 0
    }

    /// Both intrinsic dimensions are finite and greater than zero.
    open spec fn has_valid_dimensions(rendered: &RenderedFragment) -> bool {
        Self::positive_finite_f32(rendered.intrinsic_width_px)
            && Self::positive_finite_f32(rendered.intrinsic_height_px)
    }
}

} // verus!
