//! Native Vulkan submission cadence during production page transitions.

use crate::MainWindow;
use crate::graphics::{self, FrameEvent, FrameTimings};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const PAGES: [i32; 11] = [1, 2, 8, 3, 4, 6, 9, 7, 10, 5, 0];

#[derive(Default)]
struct Measurements {
    completed: usize,
    failure: Option<String>,
    renderer: Option<String>,
    first_submission_ms: Option<f64>,
    submitted_frames: u64,
    total_submissions: u64,
    idle_submissions: Option<u64>,
    window_lifecycle_passed: bool,
    measuring: bool,
    previous_submission: Option<Instant>,
    intervals_ms: Vec<f64>,
    frame_timings: Vec<(usize, FrameTimings)>,
}

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let ui = MainWindow::new()?;
    slint::select_bundled_translation("zh-CN")?;
    // Exercise the full lighting controls instead of the unavailable-capability
    // placeholder. This isolated fixture wires navigation only and writes no hardware.
    ui.set_rgb_available(true);
    println!("ui_fixture=lighting-controls-enabled hardware_workers=disabled");
    crate::ui_callbacks::wire_navigation(&ui);
    let measurements = Rc::new(RefCell::new(Measurements::default()));
    let rendering = Rc::clone(&measurements);
    graphics::set_observer(move |event, info| {
        let mut data = rendering.borrow_mut();
        if data.renderer.is_none() {
            let report = graphics::report(info);
            println!("renderer={report}");
            data.renderer = Some(report);
        }
        if event == FrameEvent::Submitted {
            data.total_submissions += 1;
            let now = Instant::now();
            if data.first_submission_ms.is_none() {
                let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                println!("first_submission_from_ui_creation_ms={elapsed:.2}");
                data.first_submission_ms = Some(elapsed);
            }
            if data.measuring {
                data.submitted_frames += 1;
                if let Some(previous) = data.previous_submission.replace(now) {
                    data.intervals_ms
                        .push(now.duration_since(previous).as_secs_f64() * 1000.0);
                }
            }
        }
        if let FrameEvent::Timings(timings) = event
            && data.measuring
        {
            let step = data.completed;
            data.frame_timings.push((step, timings));
        }
    });
    ui.show()?;
    schedule(ui.as_weak(), Rc::clone(&measurements), 0);
    slint::run_event_loop()?;
    let mut data = measurements.borrow_mut();
    if let Some(error) = data.failure.as_ref() {
        return Err(error.clone().into());
    }
    if data.completed != PAGES.len() * 2 || data.first_submission_ms.is_none() {
        return Err("Vulkan page transition check did not complete".into());
    }
    if !data.window_lifecycle_passed || data.idle_submissions != Some(0) {
        return Err(format!(
            "window lifecycle or idle draw check failed: resize_and_restore={} idle_frames={:?}",
            data.window_lifecycle_passed, data.idle_submissions
        )
        .into());
    }
    println!("resize_and_restore=passed idle_interval_ms=1000 idle_submissions=0");
    #[cfg(feature = "snapshot-tools")]
    if let Some(path) = std::env::var_os("RAM_GRAPHICS_SMOKE_SNAPSHOT") {
        let pixels = ui.window().take_snapshot()?;
        crate::write_snapshot_ppm(&std::path::PathBuf::from(path), &pixels)?;
        println!("snapshot=written");
    }
    if data.intervals_ms.len() < PAGES.len() * 2 {
        return Err("Vulkan transitions submitted too few frames to measure".into());
    }
    data.intervals_ms.sort_by(f64::total_cmp);
    let count = data.intervals_ms.len();
    println!(
        "transitions={} final_ui_state=passed submitted_frames={} submission_interval_p50_ms={:.2} submission_interval_p95_ms={:.2} submission_interval_max_ms={:.2} gaps_over_33ms={}",
        data.completed,
        data.submitted_frames,
        data.intervals_ms[count / 2],
        data.intervals_ms[((count * 95).div_ceil(100) - 1).min(count - 1)],
        data.intervals_ms[count - 1],
        data.intervals_ms.iter().filter(|&&v| v > 33.34).count()
    );
    for step in 0..PAGES.len() * 2 {
        let frames: Vec<_> = data
            .frame_timings
            .iter()
            .filter_map(|(index, timing)| (*index == step).then_some(timing))
            .collect();
        let Some(longest) = frames.iter().max_by_key(|frame| frame.total) else {
            return Err(format!("page transition {step} produced no Vulkan stage timings").into());
        };
        let milliseconds = |duration: Duration| duration.as_secs_f64() * 1000.0;
        println!(
            "frame_stages round={} page={} samples={} max_total_ms={:.2} max_acquire_ms={:.2} max_prepare_ms={:.2} max_draw_ms={:.2} max_submit_ms={:.2} max_present_ms={:.2} longest_acquire_ms={:.2} longest_draw_ms={:.2} longest_submit_ms={:.2} longest_present_ms={:.2}",
            step / PAGES.len() + 1,
            PAGES[step % PAGES.len()],
            frames.len(),
            milliseconds(longest.total),
            milliseconds(frames.iter().map(|frame| frame.acquire).max().unwrap()),
            milliseconds(frames.iter().map(|frame| frame.prepare).max().unwrap()),
            milliseconds(frames.iter().map(|frame| frame.draw).max().unwrap()),
            milliseconds(frames.iter().map(|frame| frame.submit).max().unwrap()),
            milliseconds(frames.iter().map(|frame| frame.present).max().unwrap()),
            milliseconds(longest.acquire),
            milliseconds(longest.draw),
            milliseconds(longest.submit),
            milliseconds(longest.present),
        );
    }
    Ok(())
}

fn schedule(ui: slint::Weak<MainWindow>, measurements: Rc<RefCell<Measurements>>, step: usize) {
    slint::Timer::single_shot(
        Duration::from_millis(if step == 0 { 700 } else { 40 }),
        move || {
            let Some(window) = ui.upgrade() else {
                let _ = slint::quit_event_loop();
                return;
            };
            let page = PAGES[step % PAGES.len()];
            {
                let mut data = measurements.borrow_mut();
                data.measuring = true;
                data.previous_submission = None;
            }
            window.invoke_select_page(page);
            slint::Timer::single_shot(Duration::from_millis(460), move || {
                let mut data = measurements.borrow_mut();
                data.measuring = false;
                let valid = ui.upgrade().is_some_and(|window| {
                    let valid = window.get_selected_page() == page
                        && window.get_navigation_page() == page
                        && !window.get_page_transitioning()
                        && (window.get_page_opacity() - 1.0).abs() < 0.001;
                    if !valid {
                        eprintln!(
                            "page={page} selected={} navigation={} transitioning={} opacity={}",
                            window.get_selected_page(),
                            window.get_navigation_page(),
                            window.get_page_transitioning(),
                            window.get_page_opacity()
                        );
                    }
                    valid
                });
                if !valid {
                    data.failure
                        .get_or_insert_with(|| format!("page {page} transition did not settle"));
                }
                data.completed += 1;
                let failed = data.failure.is_some();
                let done = data.completed == PAGES.len() * 2;
                drop(data);
                if failed {
                    let _ = slint::quit_event_loop();
                } else if done {
                    check_resize(ui, measurements);
                } else {
                    schedule(ui, measurements, step + 1);
                }
            });
        },
    );
}

fn check_resize(ui: slint::Weak<MainWindow>, measurements: Rc<RefCell<Measurements>>) {
    let Some(window) = ui.upgrade() else {
        let _ = slint::quit_event_loop();
        return;
    };
    let original = window.window().size();
    let resized = slint::PhysicalSize::new(original.width + 48, original.height + 24);
    let before_resize = measurements.borrow().total_submissions;
    window.window().set_size(resized);
    slint::Timer::single_shot(Duration::from_millis(450), move || {
        let Some(window) = ui.upgrade() else {
            let _ = slint::quit_event_loop();
            return;
        };
        let resize_passed = window.window().size() == resized
            && measurements.borrow().total_submissions > before_resize;
        window.window().set_minimized(true);
        slint::Timer::single_shot(Duration::from_millis(400), move || {
            let Some(window) = ui.upgrade() else {
                let _ = slint::quit_event_loop();
                return;
            };
            let was_minimized = window.window().is_minimized();
            let before_restore = measurements.borrow().total_submissions;
            window.window().set_minimized(false);
            window.window().set_size(original);
            slint::Timer::single_shot(Duration::from_millis(500), move || {
                let Some(window) = ui.upgrade() else {
                    let _ = slint::quit_event_loop();
                    return;
                };
                let mut data = measurements.borrow_mut();
                data.window_lifecycle_passed = resize_passed
                    && was_minimized
                    && !window.window().is_minimized()
                    && window.window().size() == original
                    && data.total_submissions > before_restore;
                drop(data);
                let before_idle = measurements.borrow().total_submissions;
                slint::Timer::single_shot(Duration::from_secs(1), move || {
                    let mut data = measurements.borrow_mut();
                    data.idle_submissions = Some(data.total_submissions - before_idle);
                    let _ = slint::quit_event_loop();
                });
            });
        });
    });
}
