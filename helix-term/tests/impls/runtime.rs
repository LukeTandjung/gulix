use std::{path::PathBuf, time::Duration};

use helix_core::Rope;
use helix_term::typst_preview::{
    DocumentPreview, Presentation, RenderContext, RenderRuntime, TypstCliRenderer, TypstRenderer,
    UsvgProcessor, DEFAULT_SETTINGS,
};

fn renderer(executable: PathBuf) -> TypstCliRenderer {
    let mut renderer = TypstCliRenderer::new(executable);
    renderer
        .load_settings(DEFAULT_SETTINGS)
        .expect("bundled fonts");
    renderer
}

#[tokio::test(flavor = "multi_thread")]
async fn scheduled_work_becomes_a_rendered_presentation() {
    let root = tempfile::tempdir().expect("temporary document root");
    let source = Rope::from("$ x^2 $");
    let context = RenderContext {
        body_font_size_px: 16.0,
        foreground_rgba: [0.0, 0.0, 0.0, 1.0],
        available_width_px: 640.0,
        source_root: root.path().to_path_buf(),
    };
    let document = DocumentPreview::new(source, context.clone()).expect("document preview");
    let mut runtime = RenderRuntime::new(
        document,
        renderer(PathBuf::from("typst")),
        UsvgProcessor::new(),
    );

    runtime.schedule_missing(context);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if runtime.poll() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("render completion");

    assert!(runtime
        .presentations(None)
        .into_iter()
        .flatten()
        .any(|(_, presentation)| matches!(presentation, Presentation::Rendered { .. })));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn unrelated_edit_does_not_retry_another_fragments_failed_request() {
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
    let context = RenderContext {
        body_font_size_px: 16.0,
        foreground_rgba: [0.0, 0.0, 0.0, 1.0],
        available_width_px: 640.0,
        source_root: root.path().to_path_buf(),
    };
    let document = DocumentPreview::new(
        Rope::from("#table(columns: nope, [A], [B])\n\n$x$"),
        context.clone(),
    )
    .expect("document preview");
    let mut runtime =
        RenderRuntime::new(document, renderer(executable.clone()), UsvgProcessor::new());

    runtime.schedule_missing(context.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        while calls() < 2 {
            runtime.poll();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both initial requests");
    runtime.poll();

    runtime
        .replace_source(Rope::from("#table(columns: nope, [A], [B])\n\n$y$"))
        .expect("edited source");
    runtime.schedule_missing(context.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            runtime.poll();
            runtime.schedule_missing(context.clone());
            if calls() >= 3
                && runtime
                    .presentations(None)
                    .into_iter()
                    .flatten()
                    .any(|(_, presentation)| matches!(presentation, Presentation::Rendered { .. }))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("edited inline math only");

    runtime.schedule_missing(context);
    tokio::time::sleep(Duration::from_millis(100)).await;
    runtime.poll();
    assert_eq!(calls(), 3, "the untouched failure must not be retried");
}
