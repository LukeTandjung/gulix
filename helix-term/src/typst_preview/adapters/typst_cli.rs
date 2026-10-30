//! Typst CLI implementation of the compilation port.
//!
//! It owns settings, font extraction, source preparation, process invocation,
//! temporary files, and CLI diagnostics. SVG processing belongs to the SVG adapter.

use std::{borrow::Cow, path::PathBuf};

use crate::typst_preview::ports::{CompileRequest, TypstRenderer, TypstSettings};

/// Gulix bundles Inter Math for equations and IBM Plex Mono for text in
/// code blocks, tables, and captions. Both families are OFL-licensed.
pub const BUNDLED_FONTS: &[(&str, &[u8])] = &[
    (
        "InterMath-Regular.ttf",
        include_bytes!("../../../assets/fonts/InterMath-Regular.ttf"),
    ),
    (
        "InterMath-Bold.ttf",
        include_bytes!("../../../assets/fonts/InterMath-Bold.ttf"),
    ),
    (
        "IBMPlexMono-Regular.ttf",
        include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf"),
    ),
    (
        "IBMPlexMono-Bold.ttf",
        include_bytes!("../../../assets/fonts/IBMPlexMono-Bold.ttf"),
    ),
    (
        "IBMPlexMono-Italic.ttf",
        include_bytes!("../../../assets/fonts/IBMPlexMono-Italic.ttf"),
    ),
    (
        "IBMPlexMono-BoldItalic.ttf",
        include_bytes!("../../../assets/fonts/IBMPlexMono-BoldItalic.ttf"),
    ),
];

pub const DEFAULT_SETTINGS: TypstSettings<'static> = TypstSettings {
    fonts: BUNDLED_FONTS,
    math_font: "Inter Math",
    body_font: "IBM Plex Mono",
    math_font_scale: 1.15,
    construct_rules: true,
    inline_margin_pt: 0.0,
    block_margin_pt: 1.0,
};

#[derive(Clone)]
pub struct TypstCliRenderer {
    executable: PathBuf,
    font_dir: Option<PathBuf>,
    math_font: String,
    body_font: String,
    math_font_scale: f32,
    construct_rules: bool,
    inline_margin_pt: f32,
    block_margin_pt: f32,
}

impl TypstCliRenderer {
    pub fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            font_dir: None,
            math_font: DEFAULT_SETTINGS.math_font.into(),
            body_font: DEFAULT_SETTINGS.body_font.into(),
            math_font_scale: DEFAULT_SETTINGS.math_font_scale,
            construct_rules: DEFAULT_SETTINGS.construct_rules,
            inline_margin_pt: DEFAULT_SETTINGS.inline_margin_pt,
            block_margin_pt: DEFAULT_SETTINGS.block_margin_pt,
        }
    }
}

#[derive(Debug)]
pub enum TypstCliError {
    SettingsNotLoaded,
    Spawn(std::io::Error),
    Io(std::io::Error),
    Compile { stderr: String },
    InvalidSvg(String),
}

impl TypstRenderer for TypstCliRenderer {
    type Error = TypstCliError;

    fn load_settings(&mut self, settings: TypstSettings) -> Result<(), Self::Error> {
        let mut seed: u64 = 0xcbf_29ce_4842_2325;
        for (_, file) in settings.fonts {
            for byte in *file {
                seed ^= u64::from(*byte);
                seed = seed.wrapping_mul(0x100_0000_01b3);
            }
        }

        let dir = std::env::temp_dir().join(format!("gulix-fonts-{seed:016x}"));
        std::fs::create_dir_all(&dir).map_err(TypstCliError::Io)?;
        for (name, bytes) in settings.fonts {
            let target = dir.join(name);
            if !target.exists() {
                let staging = dir.join(format!(".{name}.staging"));
                std::fs::write(&staging, bytes).map_err(TypstCliError::Io)?;
                std::fs::rename(&staging, &target).map_err(TypstCliError::Io)?;
            }
        }

        self.font_dir = Some(dir);
        self.math_font = settings.math_font.into();
        self.body_font = settings.body_font.into();
        self.math_font_scale = settings.math_font_scale;
        self.construct_rules = settings.construct_rules;
        self.inline_margin_pt = settings.inline_margin_pt;
        self.block_margin_pt = settings.block_margin_pt;
        Ok(())
    }

    fn render(&self, request: &CompileRequest) -> Result<String, Self::Error> {
        let font_dir = self
            .font_dir
            .as_ref()
            .ok_or(TypstCliError::SettingsNotLoaded)?;
        let context = request.context;
        let rgba = &context.foreground_rgba;
        let foreground = format!(
            "rgb({}%, {}%, {}%, {}%)",
            rgba[0] * 100.0,
            rgba[1] * 100.0,
            rgba[2] * 100.0,
            rgba[3] * 100.0,
        );
        let math_rule = format!(
            "#show math.equation: set text(font: \"{}\")\n",
            self.math_font
        );
        let raw_rule = format!("#show raw: set text(font: \"{}\")\n", self.body_font);
        let (rules, font_scale): (Cow<'static, str>, f32) = match request.kind {
            crate::typst_preview::FragmentKind::InlineMath
            | crate::typst_preview::FragmentKind::DisplayMath => (
                if self.construct_rules {
                    math_rule
                } else {
                    String::new()
                }
                .into(),
                self.math_font_scale,
            ),
            crate::typst_preview::FragmentKind::CodeBlock => (
                if self.construct_rules {
                    raw_rule
                } else {
                    String::new()
                }
                .into(),
                1.0,
            ),
            crate::typst_preview::FragmentKind::Table => {
                (format!("#set table(stroke: {foreground})\n").into(), 1.0)
            }
            crate::typst_preview::FragmentKind::Image
            | crate::typst_preview::FragmentKind::Figure => ("".into(), 1.0),
        };
        let font_size_pt = (context.body_font_size_px * 0.75 * font_scale * 10.0).round() / 10.0;
        let margin_pt = if request.kind == crate::typst_preview::FragmentKind::InlineMath {
            self.inline_margin_pt
        } else {
            self.block_margin_pt
        };
        let source = format!(
            "#set page(width: auto, height: auto, margin: {margin_pt}pt, fill: none)\n#set text(font: \"{}\", size: {font_size_pt}pt, fill: {foreground})\n{rules}{}\n",
            self.body_font, request.source
        );

        let root = &context.source_root;
        let input = tempfile::Builder::new()
            .prefix(".gulix-")
            .suffix(".typ")
            .tempfile_in(root)
            .map_err(TypstCliError::Io)?;
        let output = tempfile::Builder::new()
            .prefix(".gulix-")
            .suffix(".svg")
            .tempfile_in(root)
            .map_err(TypstCliError::Io)?;
        std::fs::write(input.path(), source.as_bytes()).map_err(TypstCliError::Io)?;

        let result = std::process::Command::new(&self.executable)
            .arg("compile")
            .arg("--format")
            .arg("svg")
            .arg("--font-path")
            .arg(font_dir)
            .arg("--root")
            .arg(root)
            .arg(input.path())
            .arg(output.path())
            .output()
            .map_err(TypstCliError::Spawn)?;
        if !result.status.success() {
            return Err(TypstCliError::Compile {
                stderr: String::from_utf8_lossy(&result.stderr).trim().to_owned(),
            });
        }

        let svg = std::fs::read(output.path()).map_err(TypstCliError::Io)?;
        String::from_utf8(svg).map_err(|error| TypstCliError::InvalidSvg(error.to_string()))
    }
}

// Helix/Tinymist presents source diagnostics; this adapter creates no preview-specific UI.
