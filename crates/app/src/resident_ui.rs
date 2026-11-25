//! Main-window visibility and tray residency share Slint's event-loop state.

use crate::{MainWindow, diagnostics, graphics, tray};
use slint::{CloseRequestResponse, ComponentHandle};

pub(crate) fn run(
    ui: &MainWindow,
    start_background: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // The tray owns its own hidden message window and can start before the
    // main window is shown. A failed tray must leave an accessible application.
    // Opt-in only: Slint maps the window at its default position before the
    // off-screen move lands, which flashes a 0,0 window for ~200 ms. The prewarm
    // itself works (page 2 18.6 -> 5.3 ms); the windowing must be solved first.
    if std::env::var_os("HIMARI_PREWARM").is_some_and(|value| value == "1")
        && let Err(error) = crate::prewarm::run(ui)
    {
        diagnostics::log_action(&format!("prewarm failed: {error}"));
    }
    let tray = match tray::Tray::start(ui.as_weak()) {
        Ok(tray) => tray,
        Err(error) => {
            diagnostics::log_action(&format!("tray unavailable: {error}"));
            return Ok(ui.run()?);
        }
    };
    ui.window().on_close_requested(|| {
        diagnostics::log_action("resident window hidden");
        CloseRequestResponse::HideWindow
    });
    let result = (|| {
        if !start_background {
            ui.show()?;
            crate::window_icon::apply(ui);
        }
        // The native tray thread requests quit when the user selects Exit.
        // Keeping the event loop alive does not require showing the main window.
        slint::run_event_loop_until_quit()
    })();
    tray.shutdown();
    Ok(result?)
}

pub(crate) fn smoke(start_background: bool) -> Result<(), Box<dyn std::error::Error>> {
    let ui = MainWindow::new()?;
    slint::select_bundled_translation("zh-CN")?;
    println!("tray_fixture=resident-ui hardware_workers=disabled input_hooks=disabled");
    let frame_path = std::env::var_os("NEO_CONTROL_FRAME_LOG").map(std::path::PathBuf::from);
    let mut submitted = 0_u64;
    graphics::set_observer(move |event, info| {
        if event != graphics::FrameEvent::Submitted {
            return;
        }
        submitted += 1;
        if submitted == 1
            && let Some(path) = frame_path.as_ref()
        {
            let _ = std::fs::write(path, graphics::report(info));
        }
        println!("tray_frame_submitted={submitted}");
    });
    run(&ui, start_background)?;
    // Diagnostics records the selected adapter only once a real frame submits.
    println!(
        "tray_lifecycle=exited renderer={}",
        graphics::Renderer.label()
    );
    Ok(())
}
