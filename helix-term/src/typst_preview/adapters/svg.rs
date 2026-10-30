//! `usvg`/`xmltree` implementation of the app-owned SVG operations.
//!
//! Successful fragments have well-formed SVG with finite, positive canvas dimensions
//! matching their intrinsic dimensions. Integration tests cover these external guarantees.

use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use xmltree::{Element, XMLNode};

use crate::typst_preview::ports::{InkBounds, RenderedFragment, SvgProcessor};

#[derive(Clone, Copy)]
pub struct UsvgProcessor;

impl UsvgProcessor {
    pub fn new() -> Self {
        Self
    }
}

/// Retains the parsed `usvg` tree for ink geometry and the mutable XML tree for edits.
pub struct UsvgDocument {
    tree: usvg::Tree,
    svg: Element,
}

#[derive(Debug)]
pub enum SvgError {
    InvalidSvg(String),
}

impl SvgProcessor for UsvgProcessor {
    type Document = UsvgDocument;
    type Error = SvgError;

    /// Parses raw SVG with `usvg`, resolves local resources, and normalises it
    /// into a mutable XML document. The original tree remains available for bounds.
    fn simplify(&self, svg: &[u8], source_root: &PathBuf) -> Result<Self::Document, Self::Error> {
        let mut options = usvg::Options::default();
        options.resources_dir = Some(source_root.to_path_buf());
        let tree = usvg::Tree::from_data(svg, &options)
            .map_err(|error| SvgError::InvalidSvg(error.to_string()))?;
        let svg = tree.to_string(&usvg::WriteOptions::default());
        let svg = Element::parse(svg.as_bytes())
            .map_err(|error| SvgError::InvalidSvg(error.to_string()))?;
        Ok(UsvgDocument { tree, svg })
    }

    /// Replaces embedded base64 SVG images with ordinary XML groups. Nested
    /// vectors are expanded iteratively; unsupported raster images and excessive
    /// nesting are rejected. The `usvg` tree used for ink bounds is left unchanged.
    fn expand_images(&self, mut document: Self::Document) -> Result<Self::Document, Self::Error> {
        let mut pending = vec![(0, &mut document.svg)];

        while let Some((depth, element)) = pending.pop() {
            for child in &mut element.children {
                let XMLNode::Element(child_element) = child else {
                    continue;
                };
                if child_element.name != "image" {
                    pending.push((depth, child_element));
                    continue;
                }
                if depth > 16 {
                    return Err(SvgError::InvalidSvg(
                        "nested SVG image depth exceeds 16".into(),
                    ));
                }

                let href = child_element
                    .attributes
                    .get("href")
                    .or_else(|| child_element.attributes.get("xlink:href"))
                    .cloned();
                let encoded = href
                    .as_deref()
                    .and_then(|href| href.strip_prefix("data:image/svg+xml;base64,"))
                    .ok_or_else(|| {
                        SvgError::InvalidSvg(
                            "raster images are not supported by the Tern SVG path".into(),
                        )
                    })?;
                let encoded: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
                let bytes = BASE64
                    .decode(encoded.as_bytes())
                    .map_err(|error| SvgError::InvalidSvg(error.to_string()))?;
                let nested = String::from_utf8(bytes)
                    .map_err(|error| SvgError::InvalidSvg(error.to_string()))?;
                let nested = Element::parse(nested.as_bytes())
                    .map_err(|error| SvgError::InvalidSvg(error.to_string()))?;

                let x = child_element
                    .attributes
                    .get("x")
                    .and_then(|value| value.parse::<f32>().ok())
                    .unwrap_or(0.0);
                let y = child_element
                    .attributes
                    .get("y")
                    .and_then(|value| value.parse::<f32>().ok())
                    .unwrap_or(0.0);
                let mut group = Element::new("g");
                if x != 0.0 || y != 0.0 {
                    group
                        .attributes
                        .insert("transform".into(), format!("translate({x} {y})"));
                }
                group.children = nested.children;
                *child_element = group;
                pending.push((depth + 1, child_element));
            }
        }

        Ok(document)
    }

    /// Unions the absolute, stroke-inclusive bounds of every root drawing node.
    /// Returns `None` when the document has no measurable visible ink.
    fn ink_bounds(&self, document: &Self::Document) -> Option<InkBounds> {
        let mut bounds: Option<InkBounds> = None;
        for node in document.tree.root().children() {
            let rect = node.abs_stroke_bounding_box();
            let next = InkBounds {
                left: rect.left(),
                top: rect.top(),
                right: rect.right(),
                bottom: rect.bottom(),
            };
            bounds = Some(match bounds {
                None => next,
                Some(current) => InkBounds {
                    left: current.left.min(next.left),
                    top: current.top.min(next.top),
                    right: current.right.max(next.right),
                    bottom: current.bottom.max(next.bottom),
                },
            });
        }
        bounds
    }

    /// Serializes the document without changing its canvas and takes the
    /// intrinsic dimensions from the original `usvg` canvas.
    fn into_fragment(&self, document: Self::Document) -> RenderedFragment {
        let size = document.tree.size();
        let mut bytes = Vec::new();
        document
            .svg
            .write(&mut bytes)
            .expect("writing XML to a byte vector cannot fail");
        let svg = String::from_utf8(bytes).expect("xmltree emits UTF-8");
        RenderedFragment {
            svg,
            intrinsic_width_px: size.width(),
            intrinsic_height_px: size.height(),
        }
    }

    /// Trims side slack around visible ink. If ink extends above the canvas,
    /// scales the drawing about its lowest ink point to avoid clipping without
    /// moving that baseline. Dimensions come from the rewritten canvas.
    fn fit_canvas_to_ink(
        &self,
        document: Self::Document,
        ink: InkBounds,
    ) -> Result<RenderedFragment, Self::Error> {
        const MARGIN_PX: f32 = 0.5;
        let InkBounds {
            left,
            top,
            right,
            bottom,
        } = ink;
        if !(right > left && bottom > top && bottom > 0.0) {
            return Err(SvgError::InvalidSvg(
                "ink bounds are empty, inverted, or above the canvas baseline".into(),
            ));
        }

        let scale = if top < 0.0 {
            (bottom / (bottom - top)).clamp(f32::MIN_POSITIVE, 1.0)
        } else {
            1.0
        };
        // Keeping the lowest ink point fixed means ty = bottom * (1 - scale).
        let shift_x = MARGIN_PX - scale * left;
        let shift_y = bottom * (1.0 - scale);
        let width = (scale * (right - left) + 2.0 * MARGIN_PX).max(1.0);
        let height = (bottom + MARGIN_PX).max(1.0);
        let transform = if (scale - 1.0).abs() > f32::EPSILON {
            format!("translate({shift_x} {shift_y}) scale({scale})")
        } else if shift_x != 0.0 || shift_y != 0.0 {
            format!("translate({shift_x} {shift_y})")
        } else {
            String::new()
        };

        let mut svg = document.svg;
        if svg.name != "svg" {
            return Err(SvgError::InvalidSvg(
                "simplified SVG root is not <svg>".into(),
            ));
        }
        if !transform.is_empty() {
            let mut group = Element::new("g");
            group.attributes.insert("transform".into(), transform);
            group.children = std::mem::take(&mut svg.children);
            svg.children = vec![XMLNode::Element(group)];
        }
        svg.attributes.insert("width".into(), width.to_string());
        svg.attributes.insert("height".into(), height.to_string());

        let mut bytes = Vec::new();
        svg.write(&mut bytes)
            .map_err(|error| SvgError::InvalidSvg(error.to_string()))?;
        let svg =
            String::from_utf8(bytes).map_err(|error| SvgError::InvalidSvg(error.to_string()))?;
        Ok(RenderedFragment {
            svg,
            intrinsic_width_px: width,
            intrinsic_height_px: height,
        })
    }
}
