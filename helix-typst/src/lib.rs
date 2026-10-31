//! Inline Typst previews, separated by ownership rather than by process step.
//!
//! `app` owns the document workflow, `core` owns pure decisions, `ports` declares
//! rendering, `adapters` implements it, and `api` exposes asynchronous composition.

mod adapters;
mod api;
mod app;
mod core;
mod ports;

pub use adapters::{
    SvgError, TypstCliError, TypstCliRenderer, UsvgProcessor, BUNDLED_FONTS, DEFAULT_SETTINGS,
};
pub use api::run;
pub use app::{DocumentPreview, Presentation, RenderError, RenderRequest, RenderRuntime};
pub use core::{
    FragmentId, FragmentKind, InvalidFragments, RenderContext, TerminalLayout,
    HELIX_OVERLAY_CLEARANCE, HELIX_RESERVED_ROWS,
};
pub use ports::{
    CanvasFit, CompileRequest, InkBounds, RenderedFragment, SvgProcessor, TypstRenderer,
    TypstSettings,
};
