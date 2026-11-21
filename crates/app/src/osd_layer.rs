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

/// Primary monitor work area in physical pixels, excluding the taskbar.
///
/// The vendor overlay anchors to the screen, not to another window, so the card
/// stays in the same place whether or not the control center is open.
#[cfg(windows)]
pub(crate) fn work_area() -> Option<(i32, i32, i32, i32)> {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: SPI_GETWORKAREA writes one RECT into the caller-provided buffer
    // and takes no other output; the pointer is valid for the call.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            (&mut rect) as *mut RECT as *mut c_void,
            0,
        )
    };
    (ok != 0).then_some((rect.left, rect.top, rect.right, rect.bottom))
}

#[cfg(not(windows))]
pub(crate) fn work_area() -> Option<(i32, i32, i32, i32)> {
    None
}
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
    pub width: i32,
    pub height: i32,
    pub scale: f32,
    /// `crate::osd_glyph::Glyph` identifier for the event.
    pub glyph: i32,
    /// Backlight or brightness step, 0..=4, or negative to hide the bar.
    pub level: f32,
    /// Keep the glyph turning while the card is held.
    pub spin: bool,
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

    /// Signed distance to a rounded-rectangle outline, used for the hairline.
    fn coverage_distance(x: f32, y: f32, w: f32, h: f32, radius: f32) -> f32 {
        let cx = (x - w * 0.5).abs() - (w * 0.5 - radius);
        let cy = (y - h * 0.5).abs() - (h * 0.5 - radius);
        let outside = cx.max(0.0).hypot(cy.max(0.0));
        outside + cx.max(cy).min(0.0) - radius
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

    /// Colours and measurements for one composed frame.
    #[derive(Clone, Copy)]
    pub(crate) struct FrameStyle {
        pub panel: [f32; 3],
        pub accent: [f32; 3],
        pub title: [f32; 3],
        pub detail: [f32; 3],
        pub glyph: i32,
        pub level: f32,
    }

    pub(crate) struct Overlay {
        hwnd: HWND,
        panel: Surface,
        mask: Surface,
        origin: std::cell::Cell<(i32, i32)>,
        width: i32,
        height: i32,
        scale: f32,
        /// Card, border, text and level bar, composed once per presentation.
        base: Vec<u32>,
        style: FrameStyle,
    }

    impl Overlay {
        pub(crate) fn new(width: i32, height: i32, scale: f32) -> Option<Self> {
            let panel = Surface::new(width, height)?;
            let mask = Surface::new(width, height)?;
            let class = wide("STATIC");
            // SAFETY: the class name is a predefined control class used as a
            // top-level popup, every style flag is a documented layered-window
            // combination, and no parent/instance handles are required.
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    class.as_ptr(),
                    wide("HIMARI OSD").as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    width,
                    height,
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
                width,
                height,
                scale,
                base: vec![0; (width * height) as usize],
                style: FrameStyle {
                    panel: [0.0; 3],
                    accent: [0.0; 3],
                    title: [0.0; 3],
                    detail: [0.0; 3],
                    glyph: 0,
                    level: -1.0,
                },
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
            let margin = (66.0 * self.scale) as i32;
            let card_w = self.width - margin - (16.0 * self.scale) as i32;
            self.draw_text(TextSpec {
                text: title,
                px_size: (22.0 * self.scale) as i32,
                weight: FW_SEMIBOLD as i32,
                face: "Bahnschrift",
                left: margin,
                top: (10.0 * self.scale) as i32,
                width: card_w,
                height: (30.0 * self.scale) as i32,
            });
            self.draw_text(TextSpec {
                text: detail,
                px_size: (15.0 * self.scale) as i32,
                weight: 400,
                face: "Microsoft YaHei UI",
                left: margin,
                top: (36.0 * self.scale) as i32,
                width: card_w,
                height: (24.0 * self.scale) as i32,
            });
        }

        fn coverage(mask: &[u32], index: usize) -> f32 {
            let pixel = mask[index];
            let max = (pixel & 0xff)
                .max((pixel >> 8) & 0xff)
                .max((pixel >> 16) & 0xff);
            max as f32 / 255.0
        }

        /// Compose the static layers, then draw and present one frame.
        ///
        /// Everything that does not move during the reveal — panel, border,
        /// text and level bar — is baked into `base` once. Held frames that
        /// animate the glyph copy `base` and redraw only the icon box, which
        /// keeps the per-frame cost at a few hundred pixels instead of the
        /// whole surface.
        pub(crate) fn compose(&mut self, reveal: f32, style: FrameStyle) {
            self.style = style;
            let FrameStyle {
                panel,
                accent,
                title,
                detail,
                level,
                ..
            } = style;
            let offset = (1.0 - reveal) * 10.0 * self.scale;
            let card_x = 8.0 * self.scale;
            let card_y = 8.0 * self.scale + offset;
            let card_w = (self.width as f32) - 16.0 * self.scale;
            let card_h = (self.height as f32) - 16.0 * self.scale;
            let radius = 14.0 * self.scale;
            let stroke = 1.9 * self.scale;

            let stride = self.width as usize;
            let mask_pixels = self.mask.pixels();
            let base = &mut self.base;
            for y in 0..self.height {
                for x in 0..self.width {
                    let index = y as usize * stride + x as usize;
                    base[index] = 0;
                    let fx = x as f32 + 0.5 - card_x;
                    let fy = y as f32 + 0.5 - card_y;
                    let coverage = rounded_coverage(fx, fy, card_w, card_h, radius);
                    if coverage <= 0.0 {
                        continue;
                    }
                    let pixel = &mut base[index];
                    let t = (fy / card_h).clamp(0.0, 1.0);
                    let tint = [
                        panel[0] * (1.0 - t) + panel[0] * 0.62 * t,
                        panel[1] * (1.0 - t) + panel[1] * 0.66 * t,
                        panel[2] * (1.0 - t) + panel[2] * 0.76 * t,
                    ];
                    blend(pixel, tint, coverage * 0.9 * reveal);
                    let border = (stroke * 0.5 + 0.5
                        - coverage_distance(fx, fy, card_w, card_h, radius).abs())
                    .clamp(0.0, 1.0);
                    if border > 0.0 {
                        blend(pixel, accent, border * 0.45 * reveal);
                    }
                    if level >= 0.0 {
                        let bar_x = card_x + 66.0 * self.scale;
                        let bar_y = card_y + card_h - 16.0 * self.scale;
                        let segment = 15.0 * self.scale;
                        let gap = 4.0 * self.scale;
                        let step = (level.round() as i32).clamp(0, 4);
                        for slot in 0..5 {
                            let bx = x as f32 + 0.5 - (bar_x + slot as f32 * (segment + gap));
                            let by = y as f32 + 0.5 - bar_y;
                            if !(0.0..segment).contains(&bx)
                                || !(0.0..3.0 * self.scale).contains(&by)
                            {
                                continue;
                            }
                            if slot == 0 || slot <= step {
                                blend(pixel, accent, 0.85 * reveal);
                            } else {
                                blend(pixel, accent, 0.18 * reveal);
                            }
                        }
                    }
                    let mask = Self::coverage(mask_pixels, index);
                    if mask > 0.0 {
                        let centre = card_h * 0.46;
                        let colour = if fy < centre { title } else { detail };
                        blend(pixel, colour, mask * reveal);
                    }
                }
            }
            self.paint(0.0);
            self.present();
        }

        /// Copy the composed background and draw the glyph at `spin`.
        fn paint(&mut self, spin: f32) {
            let stride = self.width as usize;
            let base = self.base.clone();
            self.panel.pixels().copy_from_slice(&base);
            let accent = self.style.accent;
            let glyph = crate::osd_glyph::Glyph::from_id(self.style.glyph);
            let prims = glyph.prims();
            if prims.is_empty() {
                return;
            }
            let offset = 0.0;
            let card_x = 8.0 * self.scale;
            let card_y = 8.0 * self.scale + offset;
            let card_h = (self.height as f32) - 16.0 * self.scale;
            let icon_size = 34.0 * self.scale;
            let icon_x = card_x + 16.0 * self.scale;
            let icon_y = card_y + (card_h - icon_size) * 0.5;
            let glyph_scale = icon_size / crate::osd_glyph::GLYPH_BOX;
            let stroke = 1.9 * self.scale;
            // Only the icon box needs to be visited.
            let box_left = icon_x.floor() as i32 - 1;
            let box_top = icon_y.floor() as i32 - 1;
            let box_right = (icon_x + icon_size).ceil() as i32 + 1;
            let box_bottom = (icon_y + icon_size).ceil() as i32 + 1;
            let (spin_sin, spin_cos) = spin.sin_cos();
            let half = crate::osd_glyph::GLYPH_BOX * 0.5;
            let pixels = self.panel.pixels();
            for y in box_top.max(0)..box_bottom.min(self.height) {
                for x in box_left.max(0)..box_right.min(self.width) {
                    let gx = (x as f32 + 0.5 - icon_x) / glyph_scale;
                    let gy = (y as f32 + 0.5 - icon_y) / glyph_scale;
                    let (dx, dy) = (gx - half, gy - half);
                    let (rx, ry) = if spin == 0.0 {
                        (gx, gy)
                    } else {
                        (
                            dx * spin_cos - dy * spin_sin + half,
                            dx * spin_sin + dy * spin_cos + half,
                        )
                    };
                    let mut alpha: f32 = 0.0;
                    for prim in prims {
                        let c =
                            crate::osd_glyph::coverage(prim, rx, ry, stroke / glyph_scale * 0.5);
                        alpha = alpha.max(c);
                    }
                    if alpha > 0.0 {
                        let index = y as usize * stride + x as usize;
                        blend(&mut pixels[index], accent, alpha);
                    }
                }
            }
        }

        /// Redraw the glyph at a new rotation and present the held card.
        pub(crate) fn spin_frame(&mut self, spin: f32) {
            self.paint(spin);
            self.present();
        }

        pub(crate) fn move_to(&self, x: i32, y: i32) {
            self.origin.set((x, y));
            self.present();
        }

        fn present(&self) {
            let size = SIZE {
                cx: self.width,
                cy: self.height,
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
        let mut pending = match receiver.recv() {
            Ok(presentation) => presentation,
            Err(_) => return,
        };
        let mut overlay = match Overlay::new(pending.width, pending.height, pending.scale) {
            Some(overlay) => overlay,
            None => {
                crate::diagnostics::log_action("OSD overlay window could not be created");
                return;
            }
        };
        loop {
            overlay.set_content(&pending.title, &pending.detail);
            overlay.move_to(pending.origin.0, pending.origin.1);
            let steps = 14;
            for step in 0..=steps {
                let reveal = step as f32 / steps as f32;
                overlay.compose(
                    reveal,
                    FrameStyle {
                        panel: pending.panel,
                        accent: pending.accent,
                        title: pending.title_colour,
                        detail: pending.detail_colour,
                        glyph: pending.glyph,
                        level: pending.level,
                    },
                );
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
            // Hold the card. A spinning glyph keeps redrawing its icon box at
            // roughly 60 frames per second; a static card presents once and
            // waits for the next request.
            let mut held: Option<super::Presentation> = None;
            if pending.spin {
                let hold = std::time::Duration::from_millis(2_100);
                let start = std::time::Instant::now();
                let mut turn: f32 = 0.0;
                while start.elapsed() < hold {
                    if let Ok(request) = receiver.try_recv() {
                        held = Some(request);
                        break;
                    }
                    turn += 0.25;
                    overlay.spin_frame(turn);
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }
            }
            let next = match held {
                Some(next) => next,
                None => {
                    let wait =
                        std::time::Duration::from_millis(if pending.spin { 0 } else { 2_100 });
                    match receiver.recv_timeout(wait) {
                        Ok(next) => next,
                        Err(RecvTimeoutError::Timeout) => {
                            overlay.hide();
                            match receiver.recv() {
                                Ok(next) => next,
                                Err(_) => return,
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            overlay.hide();
                            return;
                        }
                    }
                }
            };
            if next.width != overlay.width || next.height != overlay.height {
                match Overlay::new(next.width, next.height, next.scale) {
                    Some(replacement) => overlay = replacement,
                    None => return,
                }
            }
            pending = next;
        }
    }
}

#[cfg(windows)]
pub(crate) use windows_impl::{FrameStyle, Overlay, overlay_loop};

#[cfg(windows)]
pub(crate) fn smoke() -> Result<(), Box<dyn std::error::Error>> {
    let mut overlay = Overlay::new(320, 84, 1.0).ok_or("layered overlay could not be created")?;
    overlay.set_content("CAPS LOCK", "ON");
    let origin = work_area()
        .map(|(left, _, right, bottom)| (left + (right - left - 320) / 2, bottom - 84 - 24))
        .unwrap_or((600, 500));
    overlay.move_to(origin.0, origin.1);
    let steps = 24;
    for step in 0..=steps {
        let reveal = step as f32 / steps as f32;
        overlay.compose(
            reveal,
            FrameStyle {
                panel: [0.07, 0.08, 0.11],
                accent: [0.26, 0.83, 1.0],
                title: [0.96, 0.95, 0.98],
                detail: [0.65, 0.67, 0.73],
                glyph: 3,
                level: -1.0,
            },
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(std::time::Duration::from_millis(1_500));
    overlay.hide();
    std::thread::sleep(std::time::Duration::from_millis(200));
    Ok(())
}
