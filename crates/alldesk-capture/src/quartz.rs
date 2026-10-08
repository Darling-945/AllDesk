//! macOS screen capture via CoreGraphics (`CGDisplayCreateImage`).
//!
//! Strategy: the pipeline calls `next_frame()` at its own pace (it is the
//! single rate limiter, same contract as the DXGI capturer), and each call
//! takes one full-display screenshot and converts it to BGRA through a
//! reused bitmap context. Retina displays report backing-store pixels, so
//! the frame size can be 2x the display bounds in points — the input path
//! must convert pixel coordinates back to points (see the scaled adapter
//! in `alldesk-ffi`).
//!
//! Known limitation: `CGDisplayCreateImage` does not draw the system
//! cursor into the image, so `CapturedFrame::cursor` is always `None`
//! (viewer-side clicks still land where expected; only the remote pointer
//! arrow is not overlaid). ScreenCaptureKit can fix this later.
//!
//! Permission: macOS 10.15+ requires Screen Recording permission for the
//! app. `start_capture` triggers the system prompt once via
//! `CGRequestScreenCapture` and fails with instructions when denied.
//! After granting, the app must be restarted for capture to succeed.

use std::time::Instant;

use alldesk_core::{Error, Result};

use crate::capture::{
    CaptureConfig, CaptureProvider, CapturedFrame, FrameData, MonitorInfo, PixelFormat, Rect,
};

use core_graphics::color_space::CGColorSpace;
use core_graphics::context::CGContext;
use core_graphics::display::CGDisplay;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};

// Not exposed by core-graphics 0.24; values from CGImage.h.
/// `kCGImageAlphaPremultipliedFirst`
const K_CGIMAGE_ALPHA_PREMULTIPLIED_FIRST: u32 = 2;
/// `kCGBitmapByteOrder32Little`
const K_CGBITMAP_BYTE_ORDER_32_LITTLE: u32 = 2 << 12;
/// Little-endian ARGB in a u32 word == BGRA byte order in memory.
const BGRA_BITMAP_INFO: u32 = K_CGIMAGE_ALPHA_PREMULTIPLIED_FIRST | K_CGBITMAP_BYTE_ORDER_32_LITTLE;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCapture() -> bool;
}

/// Polling CoreGraphics capturer. `CGDisplayCreateImage` always returns the
/// current desktop content, so (unlike DXGI) there is no "quiet desktop"
/// edge case: the very first `next_frame()` produces a frame.
pub struct QuartzCapturer {
    config: CaptureConfig,
    display_id: u32,
    started: bool,
    start_time: Instant,
    /// Reused BGRA output buffer. The bitmap context writes into it
    /// in place; it is only reallocated (and the context recreated)
    /// when the display resolution changes.
    bgra: Vec<u8>,
    bitmap: Option<CGContext>,
    width: u32,
    height: u32,
}

impl Default for QuartzCapturer {
    fn default() -> Self {
        Self::new()
    }
}

impl QuartzCapturer {
    pub fn new() -> Self {
        Self {
            config: CaptureConfig::default(),
            display_id: 0,
            started: false,
            start_time: Instant::now(),
            bgra: Vec::new(),
            bitmap: None,
            width: 0,
            height: 0,
        }
    }

    /// (Re)create the bitmap context for `width`x`height` BGRA output,
    /// reusing the existing buffer when the size is unchanged.
    fn ensure_bitmap(&mut self, width: u32, height: u32) {
        if self.bitmap.is_some() && self.width == width && self.height == height {
            return;
        }
        let bytes_per_row = (width * 4) as usize;
        let len = bytes_per_row * height as usize;
        self.bgra.resize(len, 0);
        let space = CGColorSpace::create_device_rgb();
        // The context borrows self.bgra's allocation; it is kept alive in
        // self and only resized together with the context.
        let ctx = CGContext::create_bitmap_context(
            Some(self.bgra.as_mut_ptr() as *mut core::ffi::c_void),
            width as usize,
            height as usize,
            8,
            bytes_per_row,
            &space,
            BGRA_BITMAP_INFO,
        );
        self.bitmap = Some(ctx);
        self.width = width;
        self.height = height;
    }
}

fn display_name(display: &CGDisplay) -> String {
    if display.is_builtin() {
        "Built-in Display".to_string()
    } else {
        // CGDisplayUnitNumber -> friendly name needs IOKit; keep it stable
        // and unique for now, mirroring what dxgi's monitor_name does for
        // unnamed adapters.
        format!("Display {}", display.unit_number())
    }
}

/// VP9 requires even dimensions; crop the odd tail pixel (sub-0.1% scale
/// change, invisible in practice).
fn even_floor(v: u32) -> u32 {
    v & !1
}

#[async_trait::async_trait]
impl CaptureProvider for QuartzCapturer {
    async fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>> {
        let ids = CGDisplay::active_displays()
            .map_err(|e| Error::Capture(format!("CGGetActiveDisplayList: {e:?}")))?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let main_id = CGDisplay::main().id;
        let mut infos: Vec<MonitorInfo> = ids
            .iter()
            .map(|&id| {
                let d = CGDisplay::new(id);
                let b = d.bounds();
                MonitorInfo {
                    id,
                    name: display_name(&d),
                    // Points, matching how macOS positions windows; the
                    // captured frames themselves use backing pixels.
                    width: b.size.width as u32,
                    height: b.size.height as u32,
                    x: b.origin.x as i32,
                    y: b.origin.y as i32,
                    is_primary: id == main_id,
                }
            })
            .collect();
        // The sender pipeline captures monitors[0]; make that the main
        // display so the input coordinate mapping (which uses the main
        // display) stays consistent.
        infos.sort_by(|a, b| {
            let am = u8::from(a.id == main_id);
            let bm = u8::from(b.id == main_id);
            bm.cmp(&am).then(a.id.cmp(&b.id))
        });
        Ok(infos)
    }

    async fn start_capture(&mut self, config: CaptureConfig) -> Result<()> {
        // Trigger the TCC prompt (once) and fail with instructions when the
        // permission is missing; otherwise capture silently returns nil.
        unsafe {
            if !CGPreflightScreenCaptureAccess() {
                CGRequestScreenCapture();
                if !CGPreflightScreenCaptureAccess() {
                    return Err(Error::Capture(
                        "Screen Recording permission required: System Settings > Privacy & Security \
                         > Screen Recording > enable AllDesk, then restart the app"
                            .into(),
                    ));
                }
            }
        }

        let ids = CGDisplay::active_displays()
            .map_err(|e| Error::Capture(format!("CGGetActiveDisplayList: {e:?}")))?;
        let display_id = ids
            .iter()
            .find(|id| **id == config.monitor_id)
            .copied()
            .unwrap_or_else(|| CGDisplay::main().id);

        let display = CGDisplay::new(display_id);
        let pixels = (
            even_floor(display.pixels_wide() as u32),
            even_floor(display.pixels_high() as u32),
        );
        if pixels.0 == 0 || pixels.1 == 0 {
            return Err(Error::Capture("display reports zero size".into()));
        }
        self.ensure_bitmap(pixels.0, pixels.1);
        self.display_id = display_id;
        self.config = config;
        self.start_time = Instant::now();
        self.started = true;
        tracing::info!(
            "Starting CGDisplay capture on display {} ({}x{} pixels, {} fps)",
            display_id,
            pixels.0,
            pixels.1,
            self.config.fps
        );
        Ok(())
    }

    async fn stop_capture(&mut self) -> Result<()> {
        self.started = false;
        self.bitmap = None;
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<Option<CapturedFrame>> {
        if !self.started {
            return Err(Error::Capture("capture not started".into()));
        }
        let display = CGDisplay::new(self.display_id);
        let image = display.image().ok_or_else(|| {
            Error::Capture(
                "CGDisplayCreateImage returned nil — Screen Recording permission \
                 not granted to this app"
                    .into(),
            )
        })?;

        let width = even_floor(image.width() as u32);
        let height = even_floor(image.height() as u32);
        if width == 0 || height == 0 {
            return Ok(None);
        }
        self.ensure_bitmap(width, height);

        // The bitmap context borrows self.bgra; it is the only writer and no
        // &self.bgra alias is live during the draw.
        if let Some(ctx) = &self.bitmap {
            ctx.draw_image(
                CGRect::new(
                    &CGPoint::new(0.0, 0.0),
                    &CGSize::new(width as f64, height as f64),
                ),
                &image,
            );
        }

        Ok(Some(CapturedFrame {
            data: FrameData::Cpu(self.bgra.clone()),
            width,
            height,
            format: PixelFormat::Bgra8888,
            damage_regions: vec![Rect {
                x: 0,
                y: 0,
                width,
                height,
            }],
            timestamp: self.start_time.elapsed(),
            monitor_id: self.display_id,
            cursor: None,
        }))
    }
}

// SAFETY: CGContext is a CoreFoundation immutable-ish handle; all mutable
// state is behind &mut self like the DXGI capturer.
unsafe impl Send for QuartzCapturer {}
unsafe impl Sync for QuartzCapturer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bgra_bitmap_info_value() {
        // kCGImageAlphaPremultipliedFirst (2) | kCGBitmapByteOrder32Little
        // (2 << 12): little-endian ARGB words == BGRA bytes.
        assert_eq!(BGRA_BITMAP_INFO, 2 | (2 << 12));
    }

    #[test]
    fn test_even_floor() {
        assert_eq!(even_floor(3024), 3024);
        assert_eq!(even_floor(1511), 1510);
        assert_eq!(even_floor(1), 0);
        assert_eq!(even_floor(0), 0);
    }
}
