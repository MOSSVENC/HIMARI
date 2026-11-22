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

/// Colours and measurements for one composed frame.
#[derive(Clone, Copy)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct FrameStyle {
    pub panel: [f32; 3],
    pub accent: [f32; 3],
    pub title: [f32; 3],
    pub detail: [f32; 3],
    pub glyph: i32,
    pub level: f32,
}

/// Geometry and pixel maths for the overlay frame.
///
/// The compositor itself is Win32, but everything that decides what colour a
/// pixel becomes is plain arithmetic. Keeping it outside `windows_impl` means a
/// frame can be composed and inspected on any host, with no window, DIB section
/// or screen involved.
#[cfg_attr(not(windows), allow(dead_code))]
mod raster {
    use crate::osd_glyph::{self, GLYPH_BOX, Glyph, Prim};

    use super::FrameStyle;

    /// Surface size and display scale for one composed frame.
    #[derive(Clone, Copy)]
    pub(super) struct Geometry {
        pub(super) width: i32,
        pub(super) height: i32,
        pub(super) scale: f32,
    }

    impl Geometry {
        /// Left edge of the card, in surface pixels.
        pub(super) fn card_x(self) -> f32 {
            8.0 * self.scale
        }

        /// Top edge of the card, shifted down while the reveal runs.
        pub(super) fn card_y(self, offset: f32) -> f32 {
            8.0 * self.scale + offset
        }

        /// Card width, in surface pixels.
        pub(super) fn card_w(self) -> f32 {
            self.width as f32 - 16.0 * self.scale
        }

        /// Card height, in surface pixels.
        pub(super) fn card_h(self) -> f32 {
            self.height as f32 - 16.0 * self.scale
        }

        /// Icon origin and size, in surface pixels.
        ///
        /// The icon does not follow the reveal offset: it is painted at the
        /// settled position so a held frame never has to recompose it.
        pub(super) fn icon(self) -> (f32, f32, f32) {
            let icon_size = 34.0 * self.scale;
            let card_h = self.card_h();
            (
                self.card_x() + 16.0 * self.scale,
                self.card_y(0.0) + (card_h - icon_size) * 0.5,
                icon_size,
            )
        }

        /// Surface pixels the glyph may touch, before clipping to the surface.
        pub(super) fn glyph_box(self) -> (i32, i32, i32, i32) {
            let (icon_x, icon_y, icon_size) = self.icon();
            (
                icon_x.floor() as i32 - 1,
                icon_y.floor() as i32 - 1,
                (icon_x + icon_size).ceil() as i32 + 1,
                (icon_y + icon_size).ceil() as i32 + 1,
            )
        }
    }

    /// Rounded-rectangle coverage in 0..=1 with a one-pixel antialiased edge.
    pub(super) fn rounded_coverage(x: f32, y: f32, w: f32, h: f32, radius: f32) -> f32 {
        let cx = (x - w * 0.5).abs() - (w * 0.5 - radius);
        let cy = (y - h * 0.5).abs() - (h * 0.5 - radius);
        let outside = cx.max(0.0).hypot(cy.max(0.0));
        let distance = outside + cx.max(cy).min(0.0) - radius;
        (0.5 - distance).clamp(0.0, 1.0)
    }

    /// Signed distance to a rounded-rectangle outline, used for the hairline.
    pub(super) fn coverage_distance(x: f32, y: f32, w: f32, h: f32, radius: f32) -> f32 {
        let cx = (x - w * 0.5).abs() - (w * 0.5 - radius);
        let cy = (y - h * 0.5).abs() - (h * 0.5 - radius);
        let outside = cx.max(0.0).hypot(cy.max(0.0));
        outside + cx.max(cy).min(0.0) - radius
    }

    /// Composite one premultiplied `colour` over `pixel` at `alpha`.
    pub(super) fn blend(pixel: &mut u32, colour: [f32; 3], alpha: f32) {
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

    /// Coverage of one rendered text pixel: its strongest colour channel.
    pub(super) fn mask_coverage(mask: &[u32], index: usize) -> f32 {
        let pixel = mask[index];
        let max = (pixel & 0xff)
            .max((pixel >> 8) & 0xff)
            .max((pixel >> 16) & 0xff);
        max as f32 / 255.0
    }

    /// Which of the five level-bar stops are lit for a level in `0..=4`.
    ///
    /// The first stop is the baseline of the scale: it stays lit so the bar
    /// reads as a scale even at level 0, and every step lights one more stop.
    pub(super) fn level_stops(level: f32) -> [bool; 5] {
        let step = (level.round() as i32).clamp(0, 4);
        let mut stops = [false; 5];
        for (slot, stop) in stops.iter_mut().enumerate() {
            *stop = slot == 0 || slot as i32 <= step;
        }
        stops
    }

    /// Compose the layers that do not move during the reveal into `base`.
    ///
    /// Panel gradient, hairline border, text and level bar depend only on the
    /// geometry, the palette and the text mask, so a held frame copies this
    /// once-composed frame and redraws just the icon box.
    pub(super) fn compose_base(
        geometry: Geometry,
        reveal: f32,
        style: FrameStyle,
        mask: &[u32],
        base: &mut [u32],
    ) {
        let FrameStyle {
            panel,
            accent,
            title,
            detail,
            level,
            ..
        } = style;
        let Geometry {
            width,
            height,
            scale,
        } = geometry;
        let offset = (1.0 - reveal) * 10.0 * scale;
        let card_x = geometry.card_x();
        let card_y = geometry.card_y(offset);
        let card_w = geometry.card_w();
        let card_h = geometry.card_h();
        let radius = 14.0 * scale;
        let stroke = 1.9 * scale;
        let stops = level_stops(level);

        let stride = width as usize;
        for y in 0..height {
            for x in 0..width {
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
                    let bar_x = card_x + 66.0 * scale;
                    let bar_y = card_y + card_h - 16.0 * scale;
                    let segment = 15.0 * scale;
                    let gap = 4.0 * scale;
                    for (slot, lit) in stops.iter().enumerate() {
                        let bx = x as f32 + 0.5 - (bar_x + slot as f32 * (segment + gap));
                        let by = y as f32 + 0.5 - bar_y;
                        if !(0.0..segment).contains(&bx) || !(0.0..3.0 * scale).contains(&by) {
                            continue;
                        }
                        let alpha = if *lit { 0.85 } else { 0.18 };
                        blend(pixel, accent, alpha * reveal);
                    }
                }
                let text = mask_coverage(mask, index);
                if text > 0.0 {
                    let centre = card_h * 0.46;
                    let colour = if fy < centre { title } else { detail };
                    blend(pixel, colour, text * reveal);
                }
            }
        }
    }

    /// Strongest coverage over the glyph's outline primitives at one sample.
    pub(super) fn glyph_coverage(prims: &[Prim], x: f32, y: f32, half_width: f32) -> f32 {
        prims
            .iter()
            .map(|prim| osd_glyph::coverage(prim, x, y, half_width))
            .fold(0.0_f32, f32::max)
    }

    /// Paint the glyph over an already composed frame, touching only its box.
    pub(super) fn paint_glyph(
        geometry: Geometry,
        glyph: Glyph,
        accent: [f32; 3],
        spin: f32,
        pixels: &mut [u32],
    ) {
        let prims = glyph.prims();
        if prims.is_empty() {
            return;
        }
        let (icon_x, icon_y, icon_size) = geometry.icon();
        let glyph_scale = icon_size / GLYPH_BOX;
        let half_width = 1.9 * geometry.scale / glyph_scale * 0.5;
        let (box_left, box_top, box_right, box_bottom) = geometry.glyph_box();
        let half = GLYPH_BOX * 0.5;
        let (spin_sin, spin_cos) = spin.sin_cos();
        let stride = geometry.width as usize;
        // Only the icon box is visited; everything outside it keeps the bytes
        // of the composed base frame.
        for y in box_top.max(0)..box_bottom.min(geometry.height) {
            for x in box_left.max(0)..box_right.min(geometry.width) {
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
                let alpha = glyph_coverage(prims, rx, ry, half_width);
                if alpha > 0.0 {
                    let index = y as usize * stride + x as usize;
                    blend(&mut pixels[index], accent, alpha);
                }
            }
        }
    }
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

    use super::FrameStyle;
    use super::raster::{Geometry, compose_base, paint_glyph};

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

        /// Compose the static layers, then draw and present one frame.
        ///
        /// Everything that does not move during the reveal — panel, border,
        /// text and level bar — is baked into `base` once. Held frames that
        /// animate the glyph copy `base` and redraw only the icon box, which
        /// keeps the per-frame cost at a few hundred pixels instead of the
        /// whole surface.
        pub(crate) fn compose(&mut self, reveal: f32, style: FrameStyle) {
            self.style = style;
            let geometry = Geometry {
                width: self.width,
                height: self.height,
                scale: self.scale,
            };
            let mask = self.mask.pixels();
            compose_base(geometry, reveal, style, mask, &mut self.base);
            self.paint(0.0);
            self.present();
        }

        /// Copy the composed background and draw the glyph at `spin`.
        fn paint(&mut self, spin: f32) {
            let geometry = Geometry {
                width: self.width,
                height: self.height,
                scale: self.scale,
            };
            let base = self.base.clone();
            let accent = self.style.accent;
            let glyph = crate::osd_glyph::Glyph::from_id(self.style.glyph);
            let pixels = self.panel.pixels();
            pixels.copy_from_slice(&base);
            paint_glyph(geometry, glyph, accent, spin, pixels);
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
pub(crate) use windows_impl::{Overlay, overlay_loop};

#[cfg(windows)]
pub(crate) fn smoke() -> Result<(), Box<dyn std::error::Error>> {
    let mut overlay = Overlay::new(320, 84, 1.0).ok_or("layered overlay could not be created")?;
    overlay.set_content("CAPS LOCK", "ON");
    let origin = work_area()
        .map(|(left, _, right, bottom)| (left + (right - left - 320) / 2, bottom - 84 - 24))
        .unwrap_or((600, 500));
    overlay.move_to(origin.0, origin.1);
    // Same palette the product path uses, so the capture is theme evidence.
    let palette = crate::osd::card_palette(crate::runtime_state::theme_dark());
    let steps = 24;
    for step in 0..=steps {
        let reveal = step as f32 / steps as f32;
        overlay.compose(
            reveal,
            FrameStyle {
                panel: palette.panel,
                accent: [0.26, 0.83, 1.0],
                title: palette.title,
                detail: palette.detail,
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

#[cfg(test)]
mod tests {
    use crate::osd_glyph::{GLYPH_BOX, Glyph};

    use super::FrameStyle;
    use super::raster::{
        Geometry, blend, compose_base, glyph_coverage, level_stops, mask_coverage, paint_glyph,
        rounded_coverage,
    };

    /// Surface geometry of the 320 × 84 card at 100 % scale.
    fn geometry() -> Geometry {
        Geometry {
            width: 320,
            height: 84,
            scale: 1.0,
        }
    }

    fn surface(geometry: Geometry) -> Vec<u32> {
        vec![0; (geometry.width * geometry.height) as usize]
    }

    fn style(level: f32) -> FrameStyle {
        FrameStyle {
            panel: [0.07, 0.08, 0.11],
            accent: [0.26, 0.83, 1.0],
            title: [0.96, 0.95, 0.98],
            detail: [0.65, 0.67, 0.73],
            glyph: Glyph::Fan.id(),
            level,
        }
    }

    #[test]
    fn rounded_coverage_solid_inside() {
        // Centre and straight-edge band of a 100 × 60 card with 10 px corners.
        assert_eq!(rounded_coverage(50.0, 30.0, 100.0, 60.0, 10.0), 1.0);
        for x in [10.0_f32, 25.0, 50.0, 75.0, 90.0] {
            assert_eq!(rounded_coverage(x, 0.5, 100.0, 60.0, 10.0), 1.0, "x {x}");
        }
    }

    #[test]
    fn rounded_coverage_empty_far_outside() {
        assert_eq!(rounded_coverage(-40.0, -40.0, 100.0, 60.0, 10.0), 0.0);
        assert_eq!(rounded_coverage(150.0, 90.0, 100.0, 60.0, 10.0), 0.0);
        // One half pixel outside the straight edge is already empty.
        assert_eq!(rounded_coverage(-0.5, 30.0, 100.0, 60.0, 10.0), 0.0);
    }

    #[test]
    fn rounded_coverage_antialiases_one_pixel_at_the_edge() {
        // The AA band is one pixel wide and centred on the outline: empty half
        // a pixel outside, 0.5 on the outline, solid half a pixel inside.
        let cov = |x: f32| rounded_coverage(x, 30.0, 100.0, 60.0, 10.0);
        assert_eq!(cov(-0.5), 0.0);
        assert!(
            cov(-0.25) > 0.0 && cov(-0.25) < 1.0,
            "edge cell should be partial, got {}",
            cov(-0.25)
        );
        assert_eq!(cov(0.0), 0.5);
        assert!(
            cov(0.25) > 0.0 && cov(0.25) < 1.0,
            "edge cell should be partial, got {}",
            cov(0.25)
        );
        assert_eq!(cov(0.5), 1.0);
    }

    #[test]
    fn rounded_coverage_zero_radius_is_a_sharp_rectangle() {
        // A point one pixel inside a corner is solid with radius 0 ...
        assert_eq!(rounded_coverage(1.0, 1.0, 100.0, 60.0, 0.0), 1.0);
        assert_eq!(rounded_coverage(99.0, 59.0, 100.0, 60.0, 0.0), 1.0);
        // ... and outside the rounded shape once the corner is cut, so the
        // radius changes geometry rather than only edge softness.
        assert_eq!(rounded_coverage(1.0, 1.0, 100.0, 60.0, 10.0), 0.0);
        // The straight edge still antialiases with radius 0.
        assert_eq!(rounded_coverage(0.0, 30.0, 100.0, 60.0, 0.0), 0.5);
    }

    #[test]
    fn rounded_coverage_is_symmetric_at_corners() {
        // A point on the 45° corner arc reads the same from all four corners,
        // so the card cannot come out lopsided.
        let radius = 10.0_f32;
        let at = radius - radius / 2.0_f32.sqrt();
        let (w, h) = (100.0_f32, 60.0_f32);
        let corners = [(at, at), (w - at, at), (at, h - at), (w - at, h - at)];
        let first = rounded_coverage(corners[0].0, corners[0].1, w, h, radius);
        assert!(
            first > 0.0 && first < 1.0,
            "corner should be antialiased, got {first}"
        );
        for (x, y) in corners {
            let cov = rounded_coverage(x, y, w, h, radius);
            assert!(
                (cov - first).abs() < 1e-5,
                "({x}, {y}) = {cov}, expected {first}"
            );
        }
    }

    #[test]
    fn blend_opaque_over_transparent_yields_the_colour() {
        let mut pixel = 0;
        blend(&mut pixel, [0.2, 0.4, 0.8], 1.0);
        assert_eq!(pixel, 0xFF33_66CC);
    }

    #[test]
    fn blend_opaque_over_opaque_yields_the_source() {
        // Premultiplied compositing at alpha 1.0 replaces the destination.
        let mut pixel = 0xFF00_FF00;
        blend(&mut pixel, [1.0, 0.0, 0.0], 1.0);
        assert_eq!(pixel, 0xFFFF_0000);
    }

    #[test]
    fn blend_transparent_alpha_leaves_the_destination() {
        for alpha in [0.0, -1.0, -0.5] {
            let mut pixel = 0x8011_2233;
            blend(&mut pixel, [1.0, 0.0, 1.0], alpha);
            assert_eq!(pixel, 0x8011_2233, "alpha {alpha} changed the pixel");
        }
    }

    #[test]
    fn blend_clamps_channels_without_overflow() {
        // Out-of-range components clamp instead of wrapping the u8 fields.
        let mut pixel = 0;
        blend(&mut pixel, [2.0, -1.0, 1.5], 1.0);
        assert_eq!(pixel, 0xFFFF_00FF);
        // An alpha above 1.0 clamps too, so the shift into the alpha byte
        // cannot overflow and the all-red channel saturates.
        let mut pixel = 0xFFFF_FFFF;
        blend(&mut pixel, [1.0, 1.0, 1.0], 2.0);
        assert_eq!(pixel, 0xFFFF_FFFF);
    }

    #[test]
    fn mask_coverage_takes_the_strongest_channel() {
        // GDI draws the mask in white, so any channel may carry the coverage.
        assert_eq!(mask_coverage(&[0x00FF_FFFF], 0), 1.0);
        assert_eq!(mask_coverage(&[0], 0), 0.0);
        let half = mask_coverage(&[0x0000_8080], 0);
        assert!((half - 128.0 / 255.0).abs() < 1e-6, "got {half}");
    }

    #[test]
    fn glyph_outline_is_not_a_filled_disc() {
        // The fan is a hub ring plus blades. A filled disc means the arc
        // distance collapsed to zero, which is the bug this guards.
        let prims = Glyph::Fan.prims();
        let at = |x: f32, y: f32| glyph_coverage(prims, x, y, 0.95);
        assert!(
            at(24.0, 24.0) < 0.2,
            "centre should be empty, got {}",
            at(24.0, 24.0)
        );
        assert!(at(24.0, 13.0) > 0.5, "ring top should be stroked");
        assert!(at(8.0, 8.0) < 0.1, "corner should be empty");
        let mut lit = 0;
        for y in 0..48 {
            for x in 0..48 {
                if at(x as f32, y as f32) > 0.5 {
                    lit += 1;
                }
            }
        }
        assert!(lit < 900, "glyph covers {lit} of 2304 samples");
        assert!(
            lit > 100,
            "glyph is nearly invisible: {lit} of 2304 samples"
        );
    }

    #[test]
    fn paint_glyph_writes_only_inside_the_icon_box() {
        let geometry = Geometry {
            width: 200,
            height: 84,
            scale: 1.0,
        };
        let (icon_x, icon_y, icon_size) = geometry.icon();
        let glyph_scale = icon_size / GLYPH_BOX;
        let (left, top, right, bottom) = geometry.glyph_box();
        let background = 0xFF20_3040;
        let mut pixels = surface(geometry);
        pixels.fill(background);
        paint_glyph(geometry, Glyph::Fan, [1.0, 0.0, 1.0], 0.0, &mut pixels);

        let index = |x: i32, y: i32| y as usize * geometry.width as usize + x as usize;
        let mut painted_outside = 0;
        for y in 0..geometry.height {
            for x in 0..geometry.width {
                let inside = (left..right).contains(&x) && (top..bottom).contains(&y);
                if !inside && pixels[index(x, y)] != background {
                    painted_outside += 1;
                }
            }
        }
        assert_eq!(
            painted_outside, 0,
            "glyph painted {painted_outside} pixels outside its box"
        );

        // The hub is a ring, so the pixel nearest the glyph centre keeps the
        // background instead of turning into a filled disc.
        let centre_x = (icon_x + icon_size * 0.5 - 0.5).round() as i32;
        let centre_y = (icon_y + icon_size * 0.5 - 0.5).round() as i32;
        assert_eq!(
            pixels[index(centre_x, centre_y)],
            background,
            "the glyph centre was filled"
        );

        // A pixel on the top blade stroke (glyph point 24, 13) is covered.
        let blade_x = (icon_x + 24.0 * glyph_scale - 0.5).round() as i32;
        let blade_y = (icon_y + 13.0 * glyph_scale - 0.5).round() as i32;
        assert_ne!(
            pixels[index(blade_x, blade_y)],
            background,
            "the ring should be stroked"
        );
    }

    #[test]
    fn level_zero_lights_the_baseline_stop_only() {
        // The first stop is the always-on baseline of the scale, so level 0
        // shows exactly one lit stop and none of the four increments.
        assert_eq!(level_stops(0.0), [true, false, false, false, false]);
    }

    #[test]
    fn level_full_lights_all_five_stops() {
        assert_eq!(level_stops(4.0), [true, true, true, true, true]);
        // Above the last step the bar saturates rather than running out of
        // stops.
        assert_eq!(level_stops(7.0), [true, true, true, true, true]);
    }

    #[test]
    fn level_partial_rounds_to_one_increment_per_step() {
        // Each step adds exactly one lit stop above the baseline.
        for (level, expected) in [
            (0.0_f32, 1_usize),
            (0.4, 1),
            (0.6, 2),
            (1.0, 2),
            (2.0, 3),
            (3.0, 4),
        ] {
            let stops = level_stops(level);
            assert_eq!(
                stops.iter().filter(|stop| **stop).count(),
                expected,
                "level {level} lights the wrong number of stops"
            );
        }
    }

    #[test]
    fn base_frame_is_byte_identical_for_identical_input() {
        let geometry = geometry();
        let mask = vec![0; (geometry.width * geometry.height) as usize];
        let mut first = surface(geometry);
        let mut second = surface(geometry);
        compose_base(geometry, 1.0, style(-1.0), &mask, &mut first);
        compose_base(geometry, 1.0, style(-1.0), &mask, &mut second);
        assert_eq!(first, second, "the same content composed twice differs");
        assert!(
            first.iter().any(|pixel| pixel & 0xFF00_0000 != 0),
            "the panel was never composed"
        );

        // The equality above is not vacuous: a different reveal changes bytes.
        let mut other = surface(geometry);
        compose_base(geometry, 0.5, style(-1.0), &mask, &mut other);
        assert_ne!(first, other);
    }

    #[test]
    fn base_frame_text_comes_from_the_mask() {
        let geometry = geometry();
        let width = geometry.width as usize;
        let mut mask = vec![0; (geometry.width * geometry.height) as usize];
        let text = 20 * width + 100;
        // A fully covered glyph pixel, as GDI leaves it in the mask.
        mask[text] = 0x00FF_FFFF;
        let empty = vec![0; mask.len()];
        let mut with_text = surface(geometry);
        let mut without_text = surface(geometry);
        compose_base(geometry, 1.0, style(-1.0), &mask, &mut with_text);
        compose_base(geometry, 1.0, style(-1.0), &empty, &mut without_text);
        assert_ne!(
            with_text[text], without_text[text],
            "text pixel was not composited"
        );
        for (index, (with, without)) in with_text.iter().zip(without_text.iter()).enumerate() {
            if index != text {
                assert_eq!(with, without, "one mask pixel changed pixel {index}");
            }
        }
    }

    #[test]
    fn glyph_change_redraws_only_the_icon_box() {
        let geometry = geometry();
        let palette = style(-1.0);
        let mask = vec![0; (geometry.width * geometry.height) as usize];
        let mut base = surface(geometry);
        compose_base(geometry, 1.0, palette, &mask, &mut base);

        let mut fan = base.clone();
        let mut bolt = base.clone();
        paint_glyph(geometry, Glyph::Fan, palette.accent, 0.0, &mut fan);
        paint_glyph(geometry, Glyph::Bolt, palette.accent, 0.0, &mut bolt);

        let (left, top, right, bottom) = geometry.glyph_box();
        let stride = geometry.width as usize;
        let mut differing_inside = 0;
        let mut painted_outside = 0;
        for y in 0..geometry.height {
            for x in 0..geometry.width {
                let index = y as usize * stride + x as usize;
                if (left..right).contains(&x) && (top..bottom).contains(&y) {
                    if fan[index] != bolt[index] {
                        differing_inside += 1;
                    }
                    continue;
                }
                if fan[index] != base[index] || bolt[index] != base[index] {
                    painted_outside += 1;
                }
            }
        }
        assert_eq!(
            painted_outside, 0,
            "a glyph change touched pixels outside the icon box"
        );
        assert!(
            differing_inside > 0,
            "the two glyphs drew the same icon box"
        );
        assert!(
            fan.iter()
                .zip(base.iter())
                .filter(|(painted, plain)| painted != plain)
                .count()
                > 0,
            "the glyph was not painted at all"
        );
    }
}
