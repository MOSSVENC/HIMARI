//! Native pixel and pointer regression for the shared production bar components.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Rgba8Pixel, SharedPixelBuffer};

use crate::BarDiagnostics;

const LEVELS: [i32; 10] = [0, 25, 50, 75, 100, 75, 25, 0, -20, 120];
type Outcome = Rc<RefCell<Result<(), String>>>;

pub(crate) fn run(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&directory)?;
    let ui = BarDiagnostics::new()?;
    let submitted = Rc::new(std::cell::Cell::new(false));
    let observed = Rc::clone(&submitted);
    crate::graphics::set_observer(move |event, info| {
        if event == crate::graphics::FrameEvent::Submitted && !observed.replace(true) {
            println!("bars renderer: {}", crate::graphics::report(info));
        }
    });
    ui.show()?;
    let outcome = Rc::new(RefCell::new(Ok(())));
    schedule(ui.as_weak(), directory, 0, Rc::clone(&outcome));
    slint::run_event_loop()?;
    if !submitted.get() {
        return Err("bar window never submitted a Vulkan frame".into());
    }
    outcome.borrow().clone().map_err(Into::into)
}

fn schedule(ui: slint::Weak<BarDiagnostics>, directory: PathBuf, step: usize, outcome: Outcome) {
    let Some(window) = ui.upgrade() else {
        return;
    };
    window.set_level(LEVELS[step]);
    slint::Timer::single_shot(Duration::from_millis(360), move || {
        let result = (|| {
            let window = ui.upgrade().ok_or("bar window disappeared")?;
            let pixels = window.window().take_snapshot().map_err(|e| e.to_string())?;
            crate::write_snapshot_ppm(&directory.join(format!("bars-{step:02}.ppm")), &pixels)
                .map_err(|e| e.to_string())?;
            verify_pixels(&pixels, LEVELS[step].clamp(0, 100), true)?;
            println!("bars value={} pixel edges passed", LEVELS[step]);
            if step + 1 == LEVELS.len() {
                verify_drag(&window, &directory)?;
            }
            Ok(())
        })();
        if result.is_err() || step + 1 == LEVELS.len() {
            *outcome.borrow_mut() = result;
            let _ = slint::quit_event_loop();
        } else {
            schedule(ui, directory, step + 1, outcome);
        }
    });
}

fn verify_drag(ui: &BarDiagnostics, directory: &Path) -> Result<(), String> {
    for (channel, y) in [38.0, 82.0, 126.0].into_iter().enumerate() {
        let position = LogicalPosition::new(52.0, y);
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        for level in [0, 25, 50, 75, 100, 120, 75, 25, -20, 0] {
            let position = LogicalPosition::new(52.0 + 516.0 * level as f32 / 100.0, y);
            ui.window()
                .dispatch_event(WindowEvent::PointerMoved { position });
            if ui.get_level() != level.clamp(0, 100) {
                return Err(format!(
                    "channel={channel} drag={level} actual={}",
                    ui.get_level()
                ));
            }
            let pixels = ui.window().take_snapshot().map_err(|e| e.to_string())?;
            verify_pixels(&pixels, level.clamp(0, 100), false)?;
            if level == 50 {
                crate::write_snapshot_ppm(&directory.join(format!("drag-{channel}.ppm")), &pixels)
                    .map_err(|e| e.to_string())?;
            }
        }
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
    }
    println!(
        "bars: 30 pointer moves, three RGB channels, immediate fill and endpoint clamping passed"
    );
    Ok(())
}

fn verify_pixels(
    pixels: &SharedPixelBuffer<Rgba8Pixel>,
    level: i32,
    metrics: bool,
) -> Result<(), String> {
    let scale_x = pixels.width() as f32 / 640.0;
    let scale_y = pixels.height() as f32 / 280.0;
    let edge = 52.0 + 516.0 * level as f32 / 100.0;
    for channel in 0..3 {
        for (metric, y) in [
            (false, 38.0 + channel as f32 * 44.0),
            (true, 184.0 + channel as f32 * 28.0),
        ] {
            if metric && !metrics {
                continue;
            }
            let y = (y * scale_y) as usize;
            let row =
                &pixels.as_slice()[y * pixels.width() as usize..(y + 1) * pixels.width() as usize];
            let colored: Vec<f32> = row
                .iter()
                .enumerate()
                .filter_map(|(x, p)| {
                    let rgb = [p.r, p.g, p.b];
                    let logical_x = (x as f32 + 0.5) / scale_x;
                    ((42.0..578.0).contains(&logical_x)
                        && rgb[channel] > 220
                        && rgb[(channel + 1) % 3] < 35
                        && rgb[(channel + 2) % 3] < 35)
                        .then_some(logical_x)
                })
                .collect();
            if metric && level == 0 {
                if !colored.is_empty() {
                    return Err(format!("channel={channel} empty metric has colored pixels"));
                }
                continue;
            }
            let first = *colored.first().ok_or("expected bar pixels missing")?;
            let last = *colored.last().unwrap();
            let expected_first = if !metric && level < 2 {
                edge - 8.0
            } else {
                52.0
            };
            let expected_last = if metric { edge } else { edge + 8.0 };
            if (first - expected_first).abs() > 2.0 || (last - expected_last).abs() > 2.0 {
                return Err(format!(
                    "channel={channel} metric={metric} value={level}: pixels {first:.1}..{last:.1}, expected {expected_first:.1}..{expected_last:.1}"
                ));
            }
        }
    }
    Ok(())
}
