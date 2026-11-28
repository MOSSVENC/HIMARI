//! Native Vulkan submission cadence during production page transitions.

use crate::graphics::{self, FrameEvent, FrameTimings};
use crate::{MainWindow, Palette};
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
    /// Submission interval attributed to the transition step it fell inside.
    interval_steps: Vec<(usize, f64)>,
    frame_timings: Vec<(usize, FrameTimings)>,
    snapshot: Option<graphics::SnapshotResult>,
}

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let ui = MainWindow::new()?;
    slint::select_bundled_translation("zh-CN")?;
    // Exercise the full lighting controls instead of the unavailable-capability
    // placeholder. This isolated fixture wires navigation only and writes no hardware.
    ui.set_rgb_available(true);
    println!("ui_fixture=lighting-controls-enabled hardware_workers=disabled");
    if std::env::var("HIMARI_PREWARM").as_deref() != Ok("0") {
        crate::prewarm::run(&ui, crate::prewarm::Concealment::Solid)?;
        println!("prewarm=on");
    }
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
                    let milliseconds = now.duration_since(previous).as_secs_f64() * 1000.0;
                    let step = data.completed;
                    data.intervals_ms.push(milliseconds);
                    data.interval_steps.push((step, milliseconds));
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
    // Keep the measurement window off every monitor. Slint maps a window at its
    // default position before `set_position` applies, so park it at 2x2 first:
    // the only thing that can appear is a two-pixel dot, and the window then
    // stays at (-4000, -4000) for the whole run.
    let window = ui.window();
    let real_size = window.size();
    window.set_size(slint::LogicalSize::new(2.0, 2.0));
    window.set_position(slint::LogicalPosition::new(-4000.0, -4000.0));
    ui.show()?;
    window.set_size(real_size);
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
    if let Some(path) = std::env::var_os("HIMARI_GRAPHICS_SMOKE_SNAPSHOT") {
        let pixels = data
            .snapshot
            .take()
            .ok_or("Vulkan GPU snapshot did not complete")??;
        crate::write_snapshot_ppm(&std::path::PathBuf::from(path), &pixels)?;
        println!("snapshot=written source=vulkan-swapchain");
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
    // Attribute the longest gaps to the page whose construction caused them.
    let mut worst = data.interval_steps.clone();
    worst.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (step, milliseconds) in worst.iter().take(3) {
        println!(
            "transition_gap round={} page={} interval_ms={milliseconds:.2}",
            step / PAGES.len() + 1,
            PAGES[step % PAGES.len()],
        );
    }
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
        #[cfg(feature = "render-tracing")]
        println!(
            "text_stages round={} page={} draw_ms={:.2} measure_ms={:.2} glyph_ms={:.2} font_ms={:.2}",
            step / PAGES.len() + 1,
            PAGES[step % PAGES.len()],
            milliseconds(longest.text.draw),
            milliseconds(longest.text.measure),
            milliseconds(longest.text.glyph),
            milliseconds(longest.text.font),
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
                        && window.get_page_cache_ready()
                        && !window.get_page_transitioning()
                        && (window.get_page_opacity() - 1.0).abs() < 0.001
                        && window.get_page_offset().abs() < 0.001;
                    if !valid {
                        eprintln!(
                            "page={page} selected={} navigation={} transitioning={} opacity={} offset={} cache_ready={}",
                            window.get_selected_page(),
                            window.get_navigation_page(),
                            window.get_page_transitioning(),
                            window.get_page_opacity(),
                            window.get_page_offset(),
                            window.get_page_cache_ready()
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
                    {
                        let mut data = measurements.borrow_mut();
                        data.idle_submissions = Some(data.total_submissions - before_idle);
                    }
                    finish_with_snapshot(ui, measurements);
                });
            });
        });
    });
}

fn finish_with_snapshot(ui: slint::Weak<MainWindow>, measurements: Rc<RefCell<Measurements>>) {
    if std::env::var_os("HIMARI_GRAPHICS_SMOKE_SNAPSHOT").is_none() {
        let _ = slint::quit_event_loop();
        return;
    }
    capture_content_update(ui, Rc::clone(&measurements), None, 0);
    slint::Timer::single_shot(Duration::from_secs(3), move || {
        if measurements.borrow().snapshot.is_none() {
            measurements.borrow_mut().failure = Some("Vulkan GPU snapshot timed out".into());
            let _ = slint::quit_event_loop();
        }
    });
}

type Pixels = slint::SharedPixelBuffer<slint::Rgba8Pixel>;

fn capture_content_update(
    ui: slint::Weak<MainWindow>,
    measurements: Rc<RefCell<Measurements>>,
    baseline: Option<Pixels>,
    step: u8,
) {
    let Some(window) = ui.upgrade() else {
        measurements.borrow_mut().failure = Some("Vulkan capture window disappeared".into());
        let _ = slint::quit_event_loop();
        return;
    };
    let check_text = std::env::var_os("HIMARI_GRAPHICS_SMOKE_TEXT_CHANGES").is_some();
    window.set_cpu_temp(if step == 1 { "88°C" } else { "--°C" }.into());
    if check_text {
        let english = step == 3;
        if let Err(error) = slint::select_bundled_translation(if english { "en" } else { "zh-CN" })
        {
            measurements.borrow_mut().failure = Some(format!("Vulkan text fixture: {error}"));
            let _ = slint::quit_event_loop();
            return;
        }
        window.set_language_zh(!english);
        window
            .global::<Palette>()
            .set_blend(if step == 5 { 1.0 } else { 0.0 });
    }
    let result = Rc::clone(&measurements);
    if !graphics::request_snapshot(move |pixels| {
        let pixels = match pixels {
            Ok(pixels) => pixels,
            Err(error) => {
                result.borrow_mut().failure = Some(error);
                let _ = slint::quit_event_loop();
                return;
            }
        };
        let baseline = match baseline {
            Some(baseline) => baseline,
            None => pixels.clone(),
        };
        let changed = baseline
            .as_bytes()
            .iter()
            .zip(pixels.as_bytes())
            .filter(|(a, b)| a != b)
            .count();
        let valid = baseline.width() == pixels.width()
            && baseline.height() == pixels.height()
            && match step {
                0 => true,
                1 | 3 | 5 => changed >= 32,
                _ => changed == 0,
            };
        if !valid {
            result.borrow_mut().failure = Some(format!(
                "Vulkan content update failed at step {step}: changed_bytes={changed}"
            ));
            let _ = slint::quit_event_loop();
        } else if step == if check_text { 6 } else { 2 } {
            println!("gpu_content_updates=passed restored_pixels=exact");
            if check_text {
                println!(
                    "gpu_text_invalidation=passed telemetry=changed language=changed palette=changed restored_pixels=exact"
                );
            }
            result.borrow_mut().snapshot = Some(Ok(pixels));
            let _ = slint::quit_event_loop();
        } else {
            slint::Timer::single_shot(Duration::ZERO, move || {
                capture_content_update(ui, result, Some(baseline), step + 1);
            });
        }
    }) {
        measurements.borrow_mut().failure = Some("Vulkan snapshot already pending".into());
        let _ = slint::quit_event_loop();
        return;
    }
    window.window().request_redraw();
}
