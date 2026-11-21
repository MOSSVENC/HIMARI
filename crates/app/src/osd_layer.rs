//! Layered overlay window with real per-pixel alpha.
//!
//! The Slint/Vulkan overlay cannot be translucent on this driver: its swapchain
//! only offers `COMPOSITE_ALPHA_OPAQUE`. A layered window presenting a
//! premultiplied ARGB bitmap is composited by DWM with full per-pixel alpha
//! instead, which is also how the vendor overlay works.
//!
//! Rendering is done by hand into the bitmap: rounded panel coverage, the
//! accent bar and text coverage are composited per pixel, then handed to
//! `UpdateLayeredWindow`. GDI rasterises the glyphs into a separate mask bitmap
//! so text antialiasing comes from the font engine rather than from us.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::mpsc::Sender;

/// One overlay presentation: text, palette and where to place the card.
///
/// The fields are consumed by the Windows overlay thread only.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Presentation {
    pub title: String,
    pub detail: String,
    pub panel: [f32; 3],
    pub accent: [f32; 3],
    pub title_colour: [f32; 3],
    pub detail_colour: [f32; 3],
    pub origin: (i32, i32),
}

static REQUESTS: OnceLock<Mutex<Option<Sender<Presentation>>>> = OnceLock::new();

/// Start the overlay thread once. It owns the layered window for the life of
/// the process; presentation requests only carry text and palette.
pub(crate) fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let (sender, receiver) = std::sync::mpsc::channel::<Presentation>();
    if let Ok(mut slot) = REQUESTS.get_or_init(|| Mutex::new(None)).lock() {
        *slot = Some(sender);
    }
    #[cfg(windows)]
    {
        let result = std::thread::Builder::new()
            .name("osd-overlay".to_owned())
            .spawn(move || overlay_loop(receiver));
        if let Err(error) = result {
            crate::diagnostics::log_action(&format!("OSD overlay thread failed: {error}"));
        }
    }
    #[cfg(not(windows))]
    let _ = receiver;
}

/// Queue one presentation. Later requests replace the one on screen.
pub(crate) fn show(presentation: Presentation) {
    let Some(sender) = REQUESTS
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|slot| slot.clone())
    else {
        return;
    };
    let _ = sender.send(presentation);
}

#[cfg(not(windows))]
pub(crate) fn smoke() -> Result<(), Box<dyn std::error::Error>> {
    Err("the layered overlay is Windows-only".into())
}

#[cfg(windows)]
mod windows_impl {
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::mpsc::{Receiver, RecvTimeoutError};

    use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
    use windows_sys::Win32::Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
        CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, DEFAULT_CHARSET, DEFAULT_PITCH,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, DrawTextW, HBITMAP, HDC, HGDIOBJ, SelectObject,
        SetBkMode, SetTextColor, TRANSPARENT,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        ANTIALIASED_QUALITY, CreateFontW, FW_SEMIBOLD, HFONT, OUT_TT_PRECIS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow, ULW_ALPHA,
        UpdateLayeredWindow, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP,
    };

    pub(crate) const WIDTH: i32 = 320;
    pub(crate) const HEIGHT: i32 = 84;

    const DT_LEFT: u32 = 0x0000_0000;
    const DT_SINGLELINE: u32 = 0x0000_0020;
    const DT_NOPREFIX: u32 = 0x0000_0800;
    const DT_VCENTER: u32 = 0x0000_0004;

    struct Surface {
        dc: HDC,
        bitmap: HBITMAP,
        previous: HGDIOBJ,
        bits: *mut u32,
        stride: usize,
        height: i32,
    }

    impl Surface {
        fn new(width: i32, height: i32) -> Option<Self> {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = ptr::null_mut();
            // SAFETY: `info` describes a valid 32bpp top-down DIB, `bits`
            // receives the section pointer owned by the returned bitmap, a null
            // DC and module handle request screen- and driver-compatible
            // defaults, and the selection happens on handles created here.
            let (bitmap, dc, previous) = unsafe {
                let bitmap = CreateDIBSection(
                    ptr::null_mut(),
                    &info,
                    DIB_RGB_COLORS,
                    &mut bits,
                    ptr::null_mut(),
                    0,
                );
                let dc = CreateCompatibleDC(ptr::null_mut());
                if !bitmap.is_null() && dc.is_null() {
                    DeleteObject(bitmap);
                }
                let previous = if !bitmap.is_null() && !dc.is_null() {
                    SelectObject(dc, bitmap)
                } else {
                    ptr::null_mut()
                };
                (bitmap, dc, previous)
            };
            if bitmap.is_null() || bits.is_null() || dc.is_null() {
                return None;
            }
            Some(Self {
                dc,
                bitmap,
                previous,
                bits: bits.cast::<u32>(),
                stride: width as usize,
                height,
            })
        }

        /// Borrow the whole surface as a flat row-major slice.
        fn pixels(&mut self) -> &mut [u32] {
            // SAFETY: the section pointer is valid for `stride * height` u32
            // values, the bitmap is top-down, and `&self` guarantees no other
            // borrow of the same surface is alive.
            unsafe { std::slice::from_raw_parts_mut(self.bits, self.stride * self.height as usize) }
        }

        fn clear(&mut self) {
            self.pixels().fill(0);
        }
    }

    impl Drop for Surface {
        fn drop(&mut self) {
            // SAFETY: restores the original bitmap selection, then releases the
            // objects created in `new`.
            unsafe {
                SelectObject(self.dc, self.previous);
                DeleteObject(self.bitmap);
                DeleteDC(self.dc);
            }
        }
    }

    /// Rounded-rectangle coverage in 0..=1 with a one-pixel antialiased edge.
    fn rounded_coverage(x: f32, y: f32, w: f32, h: f32, radius: f32) -> f32 {
        let cx = (x - w * 0.5).abs() - (w * 0.5 - radius);
        let cy = (y - h * 0.5).abs() - (h * 0.5 - radius);
        let outside = cx.max(0.0).hypot(cy.max(0.0));
        let distance = outside + cx.max(cy).min(0.0) - radius;
        (0.5 - distance).clamp(0.0, 1.0)
    }

    fn blend(pixel: &mut u32, colour: [f32; 3], alpha: f32) {
        if alpha <= 0.0 {
            return;
        }
        let a = alpha.min(1.0);
        let current = *pixel;
        let dst_a = ((current >> 24) & 0xff) as f32 / 255.0;
        let dst_r = ((current >> 16) & 0xff) as f32 / 255.0;
        let dst_g = ((current >> 8) & 0xff) as f32 / 255.0;
        let dst_b = (current & 0xff) as f32 / 255.0;
        let out_a = a + dst_a * (1.0 - a);
        let out_r = colour[0] * a + dst_r * (1.0 - a);
        let out_g = colour[1] * a + dst_g * (1.0 - a);
        let out_b = colour[2] * a + dst_b * (1.0 - a);
        let clamp = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        *pixel = (clamp(out_a) << 24) | (clamp(out_r) << 16) | (clamp(out_g) << 8) | clamp(out_b);
    }

    /// One text line rasterised into the mask surface.
    struct TextSpec<'a> {
        text: &'a str,
        px_size: i32,
        weight: i32,
        face: &'a str,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    }

    pub(crate) struct Overlay {
        hwnd: HWND,
        panel: Surface,
        mask: Surface,
        origin: std::cell::Cell<(i32, i32)>,
    }

    impl Overlay {
        pub(crate) fn new() -> Option<Self> {
            let panel = Surface::new(WIDTH, HEIGHT)?;
            let mask = Surface::new(WIDTH, HEIGHT)?;
            let class = wide("STATIC");
            // SAFETY: the class name is a predefined control class used as a
            // top-level popup, every style flag is a documented layered-window
            // combination, and no parent/instance handles are required.
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    class.as_ptr(),
                    wide("RAM CONTROL CENTER OSD").as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    WIDTH,
                    HEIGHT,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                )
            };
            if hwnd.is_null() {
                return None;
            }
            // SAFETY: `hwnd` was just created and is live; showing without
            // activation keeps focus on the foreground application.
            unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
            Some(Self {
                hwnd,
                panel,
                mask,
                origin: std::cell::Cell::new((0, 0)),
            })
        }

        /// Rasterise a single text line into the mask surface.
        fn draw_text(&self, spec: TextSpec<'_>) {
            let TextSpec {
                text,
                px_size,
                weight,
                face,
                left,
                top,
                width,
                height,
            } = spec;
            let font = wide(face);
            // SAFETY: the face name is a live UTF-16 buffer and the remaining
            // arguments are documented font metrics.
            let handle: HFONT = unsafe {
                CreateFontW(
                    px_size,
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_TT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    ANTIALIASED_QUALITY as u32,
                    DEFAULT_PITCH as u32,
                    font.as_ptr(),
                )
            };
            if handle.is_null() {
                return;
            }
            // SAFETY: the mask DC and bitmap are live for the lifetime of
            // `self`; the font was created above and is freed at the end.
            unsafe {
                let previous = SelectObject(self.mask.dc, handle);
                SetBkMode(self.mask.dc, TRANSPARENT as i32);
                SetTextColor(self.mask.dc, 0x00FF_FFFF);
                let mut rect = windows_sys::Win32::Foundation::RECT {
                    left,
                    top,
                    right: left + width,
                    bottom: top + height,
                };
                let text_wide = wide(text);
                DrawTextW(
                    self.mask.dc,
                    text_wide.as_ptr(),
                    -1,
                    &mut rect,
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
                );
                SelectObject(self.mask.dc, previous);
                DeleteObject(handle);
            }
        }

        /// Rasterise the current title and detail into the mask surface.
        pub(crate) fn set_content(&mut self, title: &str, detail: &str) {
            self.mask.clear();
            self.draw_text(TextSpec {
                text: title,
                px_size: 22,
                weight: FW_SEMIBOLD as i32,
                face: "Bahnschrift",
                left: 40,
                top: 12,
                width: WIDTH - 60,
                height: 30,
            });
            self.draw_text(TextSpec {
                text: detail,
                px_size: 15,
                weight: 400,
                face: "Microsoft YaHei UI",
                left: 40,
                top: 44,
                width: WIDTH - 60,
                height: 24,
            });
        }

        fn coverage(mask: &[u32], index: usize) -> f32 {
            let pixel = mask[index];
            let max = (pixel & 0xff)
                .max((pixel >> 8) & 0xff)
                .max((pixel >> 16) & 0xff);
            max as f32 / 255.0
        }

        /// Compose one frame and push it to the compositor.
        pub(crate) fn render(
            &mut self,
            reveal: f32,
            panel: [f32; 3],
            accent: [f32; 3],
            title: [f32; 3],
            detail: [f32; 3],
        ) {
            self.panel.clear();
            let offset = (1.0 - reveal) * 10.0;
            let card_x = 8.0;
            let card_y = 8.0 + offset;
            let card_w = (WIDTH - 16) as f32;
            let card_h = (HEIGHT - 16) as f32;
            // Both surfaces are owned by this overlay and stay alive for this
            // frame; the borrow ends before the next mutation.
            let (panel_pixels, mask_pixels) = (self.panel.pixels(), self.mask.pixels());
            let stride = WIDTH as usize;
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let fx = x as f32 + 0.5 - card_x;
                    let fy = y as f32 + 0.5 - card_y;
                    let coverage = rounded_coverage(fx, fy, card_w, card_h, 13.0);
                    if coverage <= 0.0 {
                        continue;
                    }
                    let index = y as usize * stride + x as usize;
                    let pixel = &mut panel_pixels[index];
                    blend(pixel, panel, coverage * 0.88 * reveal);
                    let bar = rounded_coverage(fx - 16.0, fy - 12.0, 4.0, card_h - 24.0, 2.0);
                    if bar > 0.0 {
                        blend(pixel, accent, bar * reveal);
                    }
                    let mask = Self::coverage(mask_pixels, index);
                    if mask > 0.0 {
                        // The title sits above the detail line.
                        let centre = card_h * 0.46;
                        let colour = if fy < centre { title } else { detail };
                        blend(pixel, colour, mask * reveal);
                    }
                }
            }
            self.present();
        }

        pub(crate) fn move_to(&self, x: i32, y: i32) {
            self.origin.set((x, y));
            self.present();
        }

        fn present(&self) {
            let size = SIZE {
                cx: WIDTH,
                cy: HEIGHT,
            };
            let blend_function = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let source = POINT { x: 0, y: 0 };
            let (origin_x, origin_y) = self.origin.get();
            let destination = POINT {
                x: origin_x,
                y: origin_y,
            };
            // SAFETY: the window is live and `panel.dc` selects the bitmap that
            // holds the frame being presented; both points outlive the call.
            unsafe {
                let _ = UpdateLayeredWindow(
                    self.hwnd,
                    ptr::null_mut(),
                    &destination,
                    &size,
                    self.panel.dc,
                    &source,
                    0,
                    &blend_function,
                    ULW_ALPHA,
                );
            }
        }

        pub(crate) fn hide(&self) {
            // SAFETY: hiding a live window; no memory effects.
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
    }

    impl Drop for Overlay {
        fn drop(&mut self) {
            // SAFETY: the window was created by this value and is destroyed
            // exactly once here.
            unsafe { DestroyWindow(self.hwnd) };
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Own the layered window and replay the reveal for each request.
    pub(crate) fn overlay_loop(receiver: Receiver<super::Presentation>) {
        let Some(mut overlay) = Overlay::new() else {
            crate::diagnostics::log_action("OSD overlay window could not be created");
            return;
        };
        let mut pending = match receiver.recv() {
            Ok(presentation) => presentation,
            Err(_) => return,
        };
        loop {
            overlay.set_content(&pending.title, &pending.detail);
            overlay.move_to(pending.origin.0, pending.origin.1);
            let steps = 14;
            for step in 0..=steps {
                let reveal = step as f32 / steps as f32;
                overlay.render(
                    reveal,
                    pending.panel,
                    pending.accent,
                    pending.title_colour,
                    pending.detail_colour,
                );
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
            // Hold the card, but let a newer request cut the display short.
            match receiver.recv_timeout(std::time::Duration::from_millis(2_100)) {
                Ok(next) => {
                    pending = next;
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => overlay.hide(),
                Err(RecvTimeoutError::Disconnected) => {
                    overlay.hide();
                    return;
                }
            }
            match receiver.recv() {
                Ok(next) => pending = next,
                Err(_) => return,
            }
        }
    }
}

#[cfg(windows)]
pub(crate) use windows_impl::{Overlay, overlay_loop};

#[cfg(windows)]
pub(crate) fn smoke() -> Result<(), Box<dyn std::error::Error>> {
    let mut overlay = Overlay::new().ok_or("layered overlay could not be created")?;
    overlay.set_content("CAPS LOCK", "ON");
    overlay.move_to(600, 500);
    let steps = 24;
    for step in 0..=steps {
        let reveal = step as f32 / steps as f32;
        overlay.render(
            reveal,
            [0.07, 0.08, 0.11],
            [0.26, 0.83, 1.0],
            [0.96, 0.95, 0.98],
            [0.65, 0.67, 0.73],
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    overlay.hide();
    std::thread::sleep(std::time::Duration::from_millis(200));
    Ok(())
}
