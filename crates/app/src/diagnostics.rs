use crate::MainWindow;
use crate::graphics::{self, FrameEvent};
use crate::localization::tr;
use slint::ComponentHandle;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

pub(crate) fn install_frame_diagnostics(ui: &MainWindow) {
    ui.set_renderer_text(tr("Detecting").into());
    ui.set_renderer_ready(false);
    let ready = Rc::new(Cell::new(false));
    let ready_for_frame = Rc::clone(&ready);
    let weak = ui.as_weak();
    let path = std::env::var_os("NEO_CONTROL_FRAME_LOG").map(std::path::PathBuf::from);
    graphics::set_observer(move |event, info| {
        if event != FrameEvent::Submitted || ready_for_frame.replace(true) {
            return;
        }
        let report = graphics::report(info);
        if let Some(path) = path.as_ref() {
            let _ = std::fs::write(path, &report);
        }
        if let Some(ui) = weak.upgrade() {
            ui.set_renderer_text(format!("{} · {}", graphics::Renderer.label(), info.name).into());
            ui.set_renderer_ready(true);
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_secs(5), move || {
        if !ready.get()
            && let Some(ui) = weak.upgrade()
            && ui.window().is_visible()
        {
            ui.set_renderer_text(tr("Renderer initialization timed out").into());
        }
    });
}

pub(crate) fn log_action(message: &str) {
    let Some(path) = std::env::var_os("NEO_CONTROL_ACTION_LOG") else {
        return;
    };
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{message}");
    }
}
