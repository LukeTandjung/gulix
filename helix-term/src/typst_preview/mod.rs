//! Inline Typst previews, separated by ownership rather than by process step.
//!
//! `app` owns the document workflow, `core` owns pure decisions, `ports` declares
//! rendering, `adapters` implements it, and `impls` owns asynchronous runtime work.

mod adapters;
mod app;
mod core;
mod impls;
mod ports;

pub use adapters::{
    SvgError, TypstCliError, TypstCliRenderer, UsvgProcessor, BUNDLED_FONTS, DEFAULT_SETTINGS,
};
pub use app::{DocumentPreview, Presentation, RenderError, RenderRequest};
pub use core::{
    FragmentId, FragmentKind, InvalidFragments, RenderContext, TerminalLayout,
    HELIX_OVERLAY_CLEARANCE, HELIX_RESERVED_ROWS,
};
pub use impls::RenderRuntime;
pub use ports::{
    CanvasFit, CompileRequest, InkBounds, RenderedFragment, SvgProcessor, TypstRenderer,
    TypstSettings,
};
