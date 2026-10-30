use std::{path::PathBuf, sync::Arc};

use helix_core::Rope;
use helix_term::typst_preview::{
    CanvasFit, CompileRequest, DocumentPreview, FragmentId, FragmentKind, RenderContext,
    RenderRequest, RenderedFragment, SvgProcessor, TypstCliError, TypstCliRenderer, TypstRenderer,
    TypstSettings, UsvgProcessor, DEFAULT_SETTINGS,
};
use quickcheck::{quickcheck, QuickCheck, TestResult};

fn renderer() -> TypstCliRenderer {
    let mut renderer = TypstCliRenderer::new(PathBuf::from("typst"));
    renderer
        .load_settings(DEFAULT_SETTINGS)
        .expect("bundled fonts");
    renderer
}

fn assert_contract(rendered: &RenderedFragment) {
    let root = xmltree::Element::parse(rendered.svg.as_bytes()).expect("well-formed SVG");
    assert_eq!(root.name, "svg");
    let width: f32 = root.attributes.get("width").unwrap().parse().unwrap();
    let height: f32 = root.attributes.get("height").unwrap().parse().unwrap();
    assert!(width.is_finite() && width > 0.0);
    assert!(height.is_finite() && height > 0.0);
    assert_eq!(rendered.intrinsic_width_px, width);
    assert_eq!(rendered.intrinsic_height_px, height);
}

fn request(
    source: &str,
    source_root: PathBuf,
    canvas_fit: CanvasFit,
    kind: FragmentKind,
) -> RenderRequest {
    RenderRequest {
        id: FragmentId(0),
        kind,
        source: source.into(),
        canvas_fit,
        context: Arc::new(RenderContext {
            body_font_size_px: 16.0,
            foreground_rgba: [0.0, 0.0, 0.0, 1.0],
            available_width_px: 640.0,
            source_root,
        }),
    }
}

fn measure(renderer: &TypstCliRenderer, request: &RenderRequest) -> RenderedFragment {
    let raw = renderer
        .render(&CompileRequest {
            kind: request.kind,
            source: &request.source,
            context: &request.context,
        })
        .expect("Typst compilation");
    let svg = UsvgProcessor::new();
    let document = svg
        .simplify(raw.as_bytes(), &request.context.source_root)
        .expect("simplified SVG");
    svg.into_fragment(document)
}

fn ascii_text(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    bytes
        .iter()
        .map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char)
        .collect()
}

quickcheck! {
    fn supported_constructs_render_across_varied_contexts(
        kind: u8,
        font_size: u8,
        width: u16,
        colour: u8,
        body: u8
    ) -> TestResult {
        let root = tempfile::tempdir().expect("temporary document root");
        std::fs::write(
            root.path().join("figure.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"40\"><rect width=\"64\" height=\"40\"/></svg>",
        )
        .expect("local figure");
        let source = match kind % 6 {
            0 => "$x^2$".to_owned(),
            1 => "$ sum_(i=0)^n i $".to_owned(),
            2 => format!("```rust\nfn main() {{ println!(\"{body}\"); }}\n```"),
            3 => format!("#table(columns: {}, [Name], [Value])", 1 + body % 4),
            4 => "#image(\"figure.svg\")".to_owned(),
            _ => "#figure(image(\"figure.svg\"), caption: [Plot])".to_owned(),
        };
        let channel = f32::from(colour) / 255.0;
        let context = RenderContext {
            body_font_size_px: 8.0 + f32::from(font_size % 25),
            foreground_rgba: [channel, 1.0 - channel, channel, 1.0],
            available_width_px: 160.0 + f32::from(width % 881),
            source_root: root.path().to_path_buf(),
        };
        let mut document = DocumentPreview::new(Rope::from(source), context.clone())
            .expect("supported construct must form a document");
        let request = document
            .requests(context)
            .into_iter()
            .next()
            .expect("render request");
        let rendered = request.render(&renderer(), &UsvgProcessor::new()).expect("supported render");

        assert_contract(&rendered);
        TestResult::passed()
    }

    fn malformed_typst_always_returns_compile_failure(whitespace: u8, text: Vec<u8>) -> TestResult {
        let root = tempfile::tempdir().expect("temporary document root");
        let source = format!("#let [{}{}", " ".repeat(whitespace as usize), ascii_text(&text));
        let context = RenderContext {
            body_font_size_px: 16.0,
            foreground_rgba: [0.0, 0.0, 0.0, 1.0],
            available_width_px: 640.0,
            source_root: root.path().to_path_buf(),
        };
        let result = renderer().render(&CompileRequest {
            kind: FragmentKind::InlineMath,
            source: &source,
            context: &context,
        });

        TestResult::from_bool(matches!(
            result,
            Err(TypstCliError::Compile { .. })
        ))
    }

}

fn bundled_font_metrics(text: Vec<u8>, math: bool) -> TestResult {
    let text = ascii_text(&text);
    if text.is_empty() {
        return TestResult::discard();
    }
    let root = tempfile::tempdir().expect("temporary document root");
    let renderer = renderer();
    let (with_rule, without_rule) = if math {
        (
            format!("#show math.equation: set text(font: \"Inter Math\")\n$ \"{text}\"^2 $"),
            format!("$ \"{text}\"^2 $"),
        )
    } else {
        (
            format!("#show raw: set text(font: \"IBM Plex Mono\")\n```rust\n{text}\n```"),
            format!("```rust\n{text}\n```"),
        )
    };
    let with_rule =
        format!("#set page(width: auto, height: auto, margin: 0pt, fill: none)\n{with_rule}");
    let without_rule =
        format!("#set page(width: auto, height: auto, margin: 0pt, fill: none)\n{without_rule}");
    let bundled = measure(
        &renderer,
        &request(
            &with_rule,
            root.path().to_path_buf(),
            CanvasFit::AsTypeset,
            if math {
                FragmentKind::InlineMath
            } else {
                FragmentKind::CodeBlock
            },
        ),
    );
    let mut fallback_renderer = TypstCliRenderer::new(PathBuf::from("typst"));
    fallback_renderer
        .load_settings(TypstSettings {
            construct_rules: false,
            ..DEFAULT_SETTINGS
        })
        .expect("fallback settings");
    let fallback = measure(
        &fallback_renderer,
        &request(
            &without_rule,
            root.path().to_path_buf(),
            CanvasFit::AsTypeset,
            if math {
                FragmentKind::InlineMath
            } else {
                FragmentKind::CodeBlock
            },
        ),
    );
    assert_contract(&bundled);
    assert_contract(&fallback);

    TestResult::from_bool((bundled.intrinsic_width_px - fallback.intrinsic_width_px).abs() > 0.01)
}

#[test]
fn bundled_fonts_change_metrics_for_varied_text() {
    QuickCheck::new().quickcheck(bundled_font_metrics as fn(Vec<u8>, bool) -> TestResult);
}
