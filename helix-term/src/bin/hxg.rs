//! Native Gulix source-editor spike.

use gpui::{px, size, App, AppContext, Bounds, WindowBounds, WindowOptions};
use helix_term::gpui_frontend::Workspace;

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _runtime_context = runtime.enter();
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Gulix GPUI spike");
                cx.new(|cx| {
                    Workspace::new(path.as_deref(), window, cx).expect("failed to open document")
                })
            },
        )
        .expect("failed to open the Gulix window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}
