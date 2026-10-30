use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use helix_term::typst_preview::{RenderedFragment, SvgProcessor, UsvgProcessor};
use quickcheck::{quickcheck, TestResult};

fn assert_contract(processor: &UsvgProcessor, rendered: &RenderedFragment) {
    let root = xmltree::Element::parse(rendered.svg.as_bytes()).expect("well-formed SVG");
    assert_eq!(root.name, "svg");
    let width: f32 = root.attributes.get("width").unwrap().parse().unwrap();
    let height: f32 = root.attributes.get("height").unwrap().parse().unwrap();
    assert!(width.is_finite() && width > 0.0);
    assert!(height.is_finite() && height > 0.0);
    assert_eq!(width, rendered.intrinsic_width_px);
    assert_eq!(height, rendered.intrinsic_height_px);

    let document = processor
        .simplify(rendered.svg.as_bytes(), &PathBuf::from("."))
        .expect("output SVG must be accepted by usvg");
    drop(document);
}

quickcheck! {
    fn nested_svg_images_expand_at_varied_depths(depth: u8) -> TestResult {
        let processor = UsvgProcessor::new();
        let mut source = String::from(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"40\"><rect width=\"64\" height=\"40\"/></svg>",
        );
        for _ in 0..depth % 8 {
            source = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"40\"><image width=\"64\" height=\"40\" href=\"data:image/svg+xml;base64,{}\"/></svg>",
                BASE64.encode(source),
            );
        }

        let document = processor
            .simplify(source.as_bytes(), &PathBuf::from("."))
            .expect("generated SVG must parse");
        let document = processor
            .expand_images(document)
            .expect("generated vector images must expand");
        let rendered = processor.into_fragment(document);
        assert_contract(&processor, &rendered);

        TestResult::from_bool(!rendered.svg.contains("<image"))
    }

    fn ink_tight_output_contains_varied_rectangle_ink(
        x: i16,
        y: i16,
        width: u8,
        height: u8,
        canvas: u8
    ) -> TestResult {
        let processor = UsvgProcessor::new();
        let canvas = 32 + canvas % 97;
        let x = x % 33 - 16;
        let y = y % 33 - 16;
        let width = 1 + width % 24;
        let height = 1 + height % 24;
        let source = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{canvas}\" height=\"{canvas}\"><rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\"/></svg>",
        );
        let document = processor
            .simplify(source.as_bytes(), &PathBuf::from("."))
            .expect("generated rectangle must parse");
        let Some(ink) = processor.ink_bounds(&document) else {
            return TestResult::discard();
        };
        if ink.bottom <= 0.0 {
            return TestResult::discard();
        }

        let rendered = processor
            .fit_canvas_to_ink(document, ink)
            .expect("non-empty ink bounds must fit");
        assert_contract(&processor, &rendered);
        let reparsed = processor
            .simplify(rendered.svg.as_bytes(), &PathBuf::from("."))
            .expect("fitted SVG must parse");
        let Some(bounds) = processor.ink_bounds(&reparsed) else {
            return TestResult::discard();
        };
        let epsilon = 0.01;

        assert!(
            bounds.left >= -epsilon
                && bounds.top >= -epsilon
                && bounds.right <= rendered.intrinsic_width_px + epsilon
                && bounds.bottom <= rendered.intrinsic_height_px + epsilon,
            "ink escaped canvas: {bounds:?}; dimensions: {}x{}; SVG: {}",
            rendered.intrinsic_width_px,
            rendered.intrinsic_height_px,
            rendered.svg,
        );
        TestResult::passed()
    }
}
