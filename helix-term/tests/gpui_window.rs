//! Opt-in real-window test. Requires a working desktop and GPU.
//! Run: cargo test -p helix-term --features gpui-window-test --test gpui_window

use gpui::{px, size, AppContext, AsyncApp, Bounds, Keystroke, WindowBounds, WindowOptions};
use helix_term::gpui_frontend::Workspace;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

async fn type_keys(
    window: gpui::AnyWindowHandle,
    cx: &mut AsyncApp,
    keys: &[&str],
) -> anyhow::Result<()> {
    for key in keys {
        window.update(cx, |_, window, cx| {
            window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
        })?;
        cx.background_executor()
            .timer(Duration::from_millis(30))
            .await;
    }
    Ok(())
}

async fn await_file(path: &Path, expected: &str, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let actual = std::fs::read_to_string(path)?;
        if actual == expected {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "save did not finish: expected {expected:?}, got {actual:?}"
        );
        // Only the test observes disk this way. The application uses readiness notifications.
        cx.background_executor()
            .timer(Duration::from_millis(25))
            .await;
    }
}

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _runtime_context = runtime.enter();
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("window-test.txt");
    let original = format!(
        "one {}\ntwo\n{}",
        "wrap words ".repeat(80),
        "\te\u{301} 界 👩‍💻\n".repeat(100)
    );
    let modified = format!("z{original}");
    std::fs::write(&path, &original)?;
    let outcome = Arc::new(Mutex::new(None));
    let result = outcome.clone();
    gpui_platform::application().run(move |cx| {
        let bounds = Bounds::centered(None, size(px(800.), px(500.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Workspace::new(Some(&path), window, cx).unwrap()),
            )
            .unwrap();
        cx.activate(true);
        cx.spawn(async move |cx| {
            let test_result: anyhow::Result<()> = async {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let typed_window = window;
                typed_window.update(cx, |state, window, cx| {
                    let metrics = state.viewport_metrics();
                    let size = window.viewport_size();
                    let font = cx.text_system().resolve_font(&gpui::font("JetBrains Mono"));
                    let cell =
                        f32::from(cx.text_system().advance(font, px(14.), 'm').unwrap().width);
                    let line_height: f32 = 14. * 1.618_034;
                    assert_eq!(
                        metrics.0.height,
                        ((f32::from(size.height) - 32. - line_height - 1.) / line_height.round())
                            .floor()
                            .max(1.) as u16
                    );
                    assert_eq!(
                        metrics.0.width + metrics.0.x,
                        ((f32::from(size.width) - 2.) / cell).floor().max(1.) as u16
                    );
                })?;
                let window: gpui::AnyWindowHandle = window.into();
                type_keys(window, cx, &["4", "5", "0", "l"]).await?;
                let wrapped = typed_window.update(cx, |state, _, _| state.wrapping_metrics())?;
                anyhow::ensure!(
                    wrapped.0 > 0
                        && wrapped.1.is_some_and(|pos| pos.row > 0)
                        && wrapped.1 == wrapped.2,
                    "wrapped cursor does not match Helix: {wrapped:?}"
                );
                window.update(cx, |_, window, cx| {
                    window.dispatch_event(
                        gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                            position: gpui::point(px(150.), px(100.)),
                            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-46.))),
                            ..Default::default()
                        }),
                        cx,
                    );
                })?;
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let wrapped_top =
                    typed_window.update(cx, |state, _, _| state.viewport_metrics())?;
                anyhow::ensure!(
                    wrapped_top.1 > 0 && (wrapped_top.1 as f32 - wrapped_top.2).abs() < 0.1,
                    "wrapped top positioning failed: {wrapped_top:?}"
                );
                let wrapped = typed_window.update(cx, |state, _, _| state.wrapping_metrics())?;
                anyhow::ensure!(
                    wrapped.1 == wrapped.2 && wrapped.1.is_some(),
                    "wrapped cursor after scrolling does not match Helix: {wrapped:?}"
                );
                type_keys(window, cx, &["g", "g"]).await?;
                type_keys(window, cx, &["i", "z", "escape", ":", "w", "enter"]).await?;
                await_file(&path, &modified, cx).await?;
                type_keys(window, cx, &["u", ":", "w", "enter"]).await?;
                await_file(&path, &original, cx).await?;
                type_keys(window, cx, &["shift-u", ":", "w", "enter"]).await?;
                await_file(&path, &modified, cx).await?;
                type_keys(window, cx, &["5", "0", "g", "g"]).await?;
                let scrolled = typed_window.update(cx, |state, _, _| state.viewport_metrics())?;
                anyhow::ensure!(
                    scrolled.1 > 0 && (scrolled.1 as f32 - scrolled.2).abs() < 0.1,
                    "Helix and native scrolling disagree: {scrolled:?}"
                );
                window.update(cx, |_, window, cx| {
                    window.dispatch_event(
                        gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                            position: gpui::point(px(150.), px(100.)),
                            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-66.))),
                            ..Default::default()
                        }),
                        cx,
                    );
                })?;
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let wheel = typed_window.update(cx, |state, _, _| state.viewport_metrics())?;
                anyhow::ensure!(
                    wheel.1 > scrolled.1 && (wheel.1 as f32 - wheel.2).abs() < 0.1,
                    "wheel did not update Helix view: {scrolled:?} -> {wheel:?}"
                );
                type_keys(window, cx, &["z", "z"]).await?;
                let centered = typed_window.update(cx, |state, _, _| state.viewport_metrics())?;
                anyhow::ensure!(
                    (centered.1 as f32 - centered.2).abs() < 0.1,
                    "center command did not reach native view: {centered:?}"
                );
                type_keys(window, cx, &["g", "e", "z", "t"]).await?;
                let end = typed_window.update(cx, |state, _, _| state.viewport_metrics())?;
                anyhow::ensure!(
                    end.1 > 90 && (end.1 as f32 - end.2).abs() < 0.1,
                    "end-of-file top positioning failed: {end:?}"
                );
                Ok(())
            }
            .await;
            *result.lock().unwrap() = Some(test_result);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let result = outcome
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| anyhow::anyhow!("window test exited before completion"))?;
    result?;
    println!("GPUI window input, save, undo, redo, viewport sizing, and scrolling passed");
    Ok(())
}
