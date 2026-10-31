use std::{path::PathBuf, time::Duration};

use helix_core::Rope;
use helix_typst::{
    DocumentPreview, Presentation, RenderContext, RenderRuntime, TypstCliRenderer, TypstRenderer,
    UsvgProcessor, DEFAULT_SETTINGS,
};
use quickcheck::{quickcheck, TestResult};

fn renderer(executable: PathBuf) -> TypstCliRenderer {
    let mut renderer = TypstCliRenderer::new(executable);
    renderer
        .load_settings(DEFAULT_SETTINGS)
        .expect("bundled fonts");
    renderer
}

fn context(root: &std::path::Path) -> RenderContext {
    RenderContext {
        body_font_size_px: 16.0,
        foreground_rgba: [0.0, 0.0, 0.0, 1.0],
        available_width_px: 640.0,
        source_root: root.to_path_buf(),
    }
}

quickcheck! {
    fn scheduled_work_becomes_a_rendered_presentation(value: u8) -> TestResult {
        let root = tempfile::tempdir().expect("temporary document root");
        let source = Rope::from(format!("$ x_{value} $"));
        let mut document = DocumentPreview::new(source, context(root.path())).expect("document preview");
        let mut runtime = RenderRuntime::new(renderer(PathBuf::from("typst")), UsvgProcessor::new());
        let tokio_runtime = tokio::runtime::Runtime::new().unwrap();
        let _runtime_context = tokio_runtime.enter();

        runtime.schedule_missing(&mut document, context(root.path()));
        let completed = tokio_runtime.block_on(tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if runtime.poll(&mut document) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }));
        let rendered = document
            .presentations(None)
            .into_iter()
            .flatten()
            .any(|(_, presentation)| matches!(presentation, Presentation::Rendered { .. }));

        TestResult::from_bool(completed.is_ok() && rendered)
    }

    #[cfg(unix)]
    fn unrelated_edit_does_not_retry_another_fragments_failed_request(value: u8, edited_value: u8) -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temporary document root");
        let executable = root.path().join("counting-typst.sh");
        std::fs::write(
            &executable,
            r#"#!/bin/sh
printf 'call\n' >> "$0.calls"
for arg in "$@"; do
    case "$arg" in
        *.typ) input="$arg" ;;
    esac
done
if grep -q 'columns: nope' "$input"; then
    exit 1
fi
exec typst "$@" 2>> "$0.errors"
"#,
        )
        .expect("counting renderer");
        let mut permissions = std::fs::metadata(&executable)
            .expect("renderer metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).expect("executable renderer");
        let calls = || {
            std::fs::read_to_string(format!("{}.calls", executable.display()))
                .unwrap_or_default()
                .lines()
                .count()
        };
        let initial_math = if value % 2 == 0 { "$y$" } else { "$z$" };
        let mut document = DocumentPreview::new(
            Rope::from(format!("#table(columns: nope, [A], [B])\n\n{initial_math}")),
            context(root.path()),
        )
        .expect("document preview");
        let mut runtime = RenderRuntime::new(renderer(executable.clone()), UsvgProcessor::new());
        let tokio_runtime = tokio::runtime::Runtime::new().unwrap();
        let _runtime_context = tokio_runtime.enter();

        runtime.schedule_missing(&mut document, context(root.path()));
        let initial = tokio_runtime.block_on(tokio::time::timeout(Duration::from_secs(5), async {
            while calls() < 2 {
                runtime.poll(&mut document);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }));
        if initial.is_err() {
            return TestResult::failed();
        }
        runtime.poll(&mut document);
        document
            .replace_source(Rope::from(format!(
                "#table(columns: nope, [A], [B])\n\n{}",
                if edited_value % 2 == 0 {
                    initial_math
                } else if initial_math == "$y$" {
                    "$z$"
                } else {
                    "$y$"
                }
            )))
            .expect("edited source");
        runtime.schedule_missing(&mut document, context(root.path()));
        let edited = tokio_runtime.block_on(tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                runtime.poll(&mut document);
                runtime.schedule_missing(&mut document, context(root.path()));
                if document
                    .presentations(None)
                        .into_iter()
                        .flatten()
                        .any(|(_, presentation)| matches!(presentation, Presentation::Rendered { .. }))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }));
        let expected_calls = 3;
        runtime.poll(&mut document);
        let diagnostics = std::fs::read_to_string(format!("{}.errors", executable.display()))
            .unwrap_or_default();
        assert!(
            edited.is_ok() && calls() <= expected_calls,
            "edited={edited:?}, calls={expected_calls} expected, actual={}, diagnostics={diagnostics}",
            calls()
        );
        TestResult::passed()
    }
}
