//! Windows DXGI Desktop Duplication screen capture implementation.
//!
//! Uses the DXGI Output Duplication API (Windows 8+) to capture the desktop.
//! Creates a D3D11 device, enumerates adapters/outputs, and duplicates the
//! desktop output to obtain frame data as BGRA pixels.

use std::time::{Duration, Instant};

use tracing::{info, warn};
use windows::core::IUnknown;
use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIOutput, IDXGIOutput1, IDXGIOutputDuplication,
    IDXGIResource, DXGI_ADAPTER_DESC, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO, DXGI_OUTDUPL_POINTER_SHAPE_INFO, DXGI_OUTPUT_DESC,
};

use alldesk_core::Error;
use alldesk_core::Result;

use crate::capture::{
    CaptureConfig, CaptureProvider, CapturedFrame, CursorInfo, CursorShapeType, FrameData,
    MonitorInfo, PixelFormat,
};

/// DXGI-based desktop duplication capturer for Windows.
pub struct DxgiCapturer {
    device: Option<ID3D11Device>,
    context: Option<ID3D11DeviceContext>,
    duplication: Option<IDXGIOutputDuplication>,
    /// Output width and height extracted from DXGI_OUTPUT_DESC at start_capture.
    output_width: u32,
    output_height: u32,
    config: Option<CaptureConfig>,
    /// Instant used to compute frame timestamps relative to capture start.
    start_instant: Option<Instant>,
    /// Whether we need to reinitialize (e.g. after access lost).
    needs_reinit: bool,
    /// Cached staging texture for CPU readback, reused across frames when dimensions match.
    staging_texture: Option<ID3D11Texture2D>,
    /// Cached dimensions (width, height) of the current staging texture.
    staging_dims: (u32, u32),
    /// Target frame interval for frame rate limiting.
    frame_interval: Option<Duration>,
    /// Timestamp of the last captured frame for rate limiting.
    last_frame_time: Option<Instant>,
    /// Number of consecutive reinit attempts to avoid infinite loops.
    reinit_attempts: u32,
}

// COM interfaces are thread-safe via reference counting and the DXGI/D3D11 runtime
// handles synchronization for the APIs we use.
unsafe impl Send for DxgiCapturer {}
unsafe impl Sync for DxgiCapturer {}

impl Default for DxgiCapturer {
    fn default() -> Self {
        Self::new()
    }
}

impl DxgiCapturer {
    pub fn new() -> Self {
        Self {
            device: None,
            context: None,
            duplication: None,
            output_width: 0,
            output_height: 0,
            config: None,
            start_instant: None,
            needs_reinit: false,
            staging_texture: None,
            staging_dims: (0, 0),
            frame_interval: None,
            last_frame_time: None,
            reinit_attempts: 0,
        }
    }

    /// Return a staging texture suitable for the given dimensions and format.
    /// Reuses the cached texture when dimensions match; creates a new one otherwise.
    fn get_staging_texture(
        &mut self,
        device: &ID3D11Device,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Result<ID3D11Texture2D> {
        // Reuse cached texture when dimensions match.
        if let Some(ref tex) = self.staging_texture {
            if self.staging_dims == (width, height) {
                return Ok(tex.clone());
            }
        }

        // Dimensions changed (or no cached texture yet) — allocate a new one.
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };

        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe {
            device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                .map_err(|e| win_err(e, "CreateTexture2D (staging) failed"))?;
        }

        let staging =
            staging.ok_or_else(|| Error::Capture("Staging texture was not created".into()))?;

        self.staging_texture = Some(staging.clone());
        self.staging_dims = (width, height);
        Ok(staging)
    }
}

/// Helper to convert a `windows::core::Error` into an `alldesk_core::Error`.
fn win_err(e: windows::core::Error, context: &str) -> Error {
    Error::Capture(format!("{}: {}", context, e))
}

/// Extract dirty rectangles from DXGI frame metadata.
fn extract_dirty_rects(
    duplication: &IDXGIOutputDuplication,
    buffer_size: usize,
) -> Vec<crate::capture::Rect> {
    let mut buf = vec![0u8; buffer_size];
    let mut rects_returned = 0u32;

    let result = unsafe {
        duplication.GetFrameDirtyRects(
            buffer_size as u32,
            buf.as_mut_ptr() as *mut RECT,
            &mut rects_returned,
        )
    };

    if result.is_err() || rects_returned == 0 {
        return Vec::new();
    }

    let rect_size = std::mem::size_of::<RECT>();
    let num_rects = rects_returned as usize;
    let rects = unsafe {
        std::slice::from_raw_parts(
            buf.as_ptr() as *const RECT,
            num_rects.min(buffer_size / rect_size),
        )
    };

    rects
        .iter()
        .map(|r| crate::capture::Rect {
            x: r.left,
            y: r.top,
            width: (r.right - r.left) as u32,
            height: (r.bottom - r.top) as u32,
        })
        .collect()
}

/// Extract cursor information from DXGI frame info.
/// Returns cursor position and optionally the cursor shape image.
fn extract_cursor_info(
    duplication: &IDXGIOutputDuplication,
    frame_info: &DXGI_OUTDUPL_FRAME_INFO,
    show_cursor: bool,
) -> Option<CursorInfo> {
    if !show_cursor {
        return None;
    }

    let pos = &frame_info.PointerPosition;
    let visible = pos.Visible.as_bool();

    // Extract cursor shape if the buffer size is non-zero.
    let (shape_data, shape_width, shape_height, hot_spot_x, hot_spot_y, shape_type) =
        if frame_info.PointerShapeBufferSize > 0 {
            let buf_size = frame_info.PointerShapeBufferSize as usize;
            let mut shape_buf = vec![0u8; buf_size];
            let mut required_size: u32 = 0;
            let mut shape_info = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();

            let result = unsafe {
                duplication.GetFramePointerShape(
                    buf_size as u32,
                    shape_buf.as_mut_ptr() as *mut _,
                    &mut required_size,
                    &mut shape_info,
                )
            };

            if result.is_ok() {
                let st = match shape_info.Type {
                    1 => CursorShapeType::Monochrome,
                    2 => CursorShapeType::Color,
                    4 => CursorShapeType::MaskedColor,
                    _ => CursorShapeType::Color,
                };
                (
                    Some(shape_buf),
                    shape_info.Width,
                    shape_info.Height,
                    shape_info.HotSpot.x as u32,
                    shape_info.HotSpot.y as u32,
                    st,
                )
            } else {
                (None, 0, 0, 0, 0, CursorShapeType::Monochrome)
            }
        } else {
            (None, 0, 0, 0, 0, CursorShapeType::Monochrome)
        };

    Some(CursorInfo {
        x: pos.Position.x,
        y: pos.Position.y,
        visible,
        shape_data,
        shape_width,
        shape_height,
        hot_spot_x,
        hot_spot_y,
        shape_type,
    })
}

/// Create the D3D11 device and immediate context.
unsafe fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;

    let feature_levels = [
        D3D_FEATURE_LEVEL(0xb100), // D3D_FEATURE_LEVEL_11_1
        D3D_FEATURE_LEVEL(0xb000), // D3D_FEATURE_LEVEL_11_0
        D3D_FEATURE_LEVEL(0xa100), // D3D_FEATURE_LEVEL_10_1
        D3D_FEATURE_LEVEL(0xa000), // D3D_FEATURE_LEVEL_10_0
    ];

    unsafe {
        D3D11CreateDevice(
            None,                                                              // padapter: default adapter
            D3D_DRIVER_TYPE_HARDWARE,                                          // drivertype
            None, // software: None for hardware
            windows::Win32::Graphics::Direct3D11::D3D11_CREATE_DEVICE_FLAG(0), // flags: none
            Some(&feature_levels), // pfeaturelevels
            D3D11_SDK_VERSION, // sdkversion
            Some(&mut device), // ppdevice
            None, // pfeaturelevel
            Some(&mut context), // ppimmediatecontext
        )
        .map_err(|e| win_err(e, "D3D11CreateDevice failed"))?;
    }

    let device = device.ok_or_else(|| Error::Capture("D3D11 device was not created".into()))?;
    let context =
        context.ok_or_else(|| Error::Capture("D3D11 device context was not created".into()))?;

    Ok((device, context))
}

/// Enumerate all DXGI adapters in the system.
unsafe fn enum_adapters() -> Result<Vec<IDXGIAdapter>> {
    let factory = unsafe {
        CreateDXGIFactory1::<windows::Win32::Graphics::Dxgi::IDXGIFactory1>()
            .map_err(|e| win_err(e, "CreateDXGIFactory1 failed"))?
    };

    let mut adapters = Vec::new();
    for i in 0.. {
        match unsafe { factory.EnumAdapters(i) } {
            Ok(adapter) => adapters.push(adapter),
            Err(_) => break, // DXGI_ERROR_NOT_FOUND means no more adapters
        }
    }
    Ok(adapters)
}

/// Enumerate all outputs for a given adapter.
unsafe fn enum_outputs(adapter: &IDXGIAdapter) -> Result<Vec<IDXGIOutput>> {
    let mut outputs = Vec::new();
    for i in 0.. {
        match unsafe { adapter.EnumOutputs(i) } {
            Ok(output) => outputs.push(output),
            Err(_) => break,
        }
    }
    Ok(outputs)
}

/// Get the description of a DXGI output.
fn get_output_desc(output: &IDXGIOutput) -> windows::core::Result<DXGI_OUTPUT_DESC> {
    unsafe { output.GetDesc() }
}

/// Create the desktop duplication for a specific output using the D3D11 device.
unsafe fn create_duplication(
    device: &ID3D11Device,
    output: &IDXGIOutput,
) -> Result<IDXGIOutputDuplication> {
    // Cast the output to IDXGIOutput1 to access DuplicateOutput.
    let output1: IDXGIOutput1 = output
        .cast()
        .map_err(|e| win_err(e, "Failed to cast IDXGIOutput to IDXGIOutput1"))?;

    // DuplicateOutput takes the device as IUnknown.
    let device_unknown: IUnknown = device
        .cast()
        .map_err(|e| win_err(e, "Failed to cast ID3D11Device to IUnknown"))?;

    unsafe { output1.DuplicateOutput(&device_unknown) }
        .map_err(|e| win_err(e, "DuplicateOutput failed"))
}

/// Build a human-readable name from the adapter and output descriptions.
fn monitor_name(adapter_desc: &DXGI_ADAPTER_DESC, output_index: u32) -> String {
    let adapter_name = wide_string(&adapter_desc.Description);
    let trimmed = adapter_name.trim_end_matches('\0');
    if trimmed.is_empty() {
        format!("Monitor {} (Output {})", output_index, output_index)
    } else {
        format!("{} - Output {}", trimmed, output_index)
    }
}

/// Read a wide null-terminated string from a `[u16; N]` buffer.
fn wide_string<const N: usize>(buf: &[u16; N]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(N);
    String::from_utf16_lossy(&buf[..end])
}

#[async_trait::async_trait]
impl CaptureProvider for DxgiCapturer {
    async fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>> {
        let adapters = unsafe { enum_adapters() }?;
        let mut monitors = Vec::new();
        let mut monitor_id: u32 = 0;

        for adapter in &adapters {
            let adapter_desc = unsafe {
                adapter
                    .GetDesc()
                    .map_err(|e| win_err(e, "IDXGIAdapter::GetDesc failed"))?
            };

            let outputs = unsafe { enum_outputs(adapter) }?;
            for output in &outputs {
                let desc = get_output_desc(output)
                    .map_err(|e| win_err(e, "IDXGIOutput::GetDesc failed"))?;

                let desktop_rect = desc.DesktopCoordinates;
                let width = (desktop_rect.right - desktop_rect.left) as u32;
                let height = (desktop_rect.bottom - desktop_rect.top) as u32;

                let device_name = wide_string(&desc.DeviceName);
                let name = if device_name.is_empty() {
                    monitor_name(&adapter_desc, monitor_id)
                } else {
                    device_name
                };

                // In DXGI, the primary monitor is attached to the desktop.
                let is_primary = desc.AttachedToDesktop.as_bool();

                monitors.push(MonitorInfo {
                    id: monitor_id,
                    name,
                    width,
                    height,
                    x: desktop_rect.left,
                    y: desktop_rect.top,
                    is_primary,
                });
                monitor_id += 1;
            }
        }

        info!("Enumerated {} monitor(s)", monitors.len());
        Ok(monitors)
    }

    async fn start_capture(&mut self, config: CaptureConfig) -> Result<()> {
        info!(
            "Starting DXGI capture on monitor {} at {} fps",
            config.monitor_id, config.fps
        );

        // Create D3D11 device.
        let (device, context) = unsafe { create_device() }?;
        self.device = Some(device);
        self.context = Some(context);

        // Find the requested monitor by enumerating adapters and outputs.
        let adapters = unsafe { enum_adapters() }?;
        let mut target_output: Option<IDXGIOutput> = None;
        let mut current_id: u32 = 0;

        'outer: for adapter in &adapters {
            let outputs = unsafe { enum_outputs(adapter) }?;
            for output in outputs {
                if current_id == config.monitor_id {
                    target_output = Some(output);
                    break 'outer;
                }
                current_id += 1;
            }
        }

        let output = target_output.ok_or_else(|| {
            Error::Capture(format!(
                "Monitor {} not found during start_capture",
                config.monitor_id
            ))
        })?;

        // Store the output dimensions for frame size info.
        let desc =
            get_output_desc(&output).map_err(|e| win_err(e, "IDXGIOutput::GetDesc failed"))?;
        let desktop_rect = desc.DesktopCoordinates;
        self.output_width = (desktop_rect.right - desktop_rect.left) as u32;
        self.output_height = (desktop_rect.bottom - desktop_rect.top) as u32;

        // Create the desktop duplication.
        let device = self.device.as_ref().unwrap();
        let duplication = unsafe { create_duplication(device, &output) }?;
        self.duplication = Some(duplication);

        self.config = Some(config);
        self.start_instant = Some(Instant::now());
        self.needs_reinit = false;
        self.reinit_attempts = 0;

        // Set frame rate limiting based on config fps.
        let fps = self.config.as_ref().map(|c| c.fps).unwrap_or(30);
        if fps > 0 {
            self.frame_interval = Some(Duration::from_secs_f64(1.0 / fps as f64));
        } else {
            self.frame_interval = None;
        }
        self.last_frame_time = None;

        info!("DXGI desktop duplication initialized successfully");
        Ok(())
    }

    async fn stop_capture(&mut self) -> Result<()> {
        info!("Stopping DXGI capture");

        // Release the duplication first.
        if let Some(dup) = self.duplication.take() {
            unsafe {
                let _ = dup.ReleaseFrame();
            }
            drop(dup);
        }

        self.context = None;
        self.device = None;
        self.output_width = 0;
        self.output_height = 0;
        self.start_instant = None;
        self.config = None;
        self.needs_reinit = false;
        self.staging_texture = None;
        self.staging_dims = (0, 0);
        self.frame_interval = None;
        self.last_frame_time = None;
        self.reinit_attempts = 0;

        Ok(())
    }

    async fn next_frame(&mut self) -> Result<Option<CapturedFrame>> {
        // Handle reinitialization after DXGI_ACCESS_LOST.
        if self.needs_reinit {
            if self.reinit_attempts >= 5 {
                return Err(Error::Capture(
                    "Max reinit attempts reached after DXGI access lost".into(),
                ));
            }
            self.reinit_attempts += 1;
            info!(
                "Attempting DXGI reinitialization (attempt {})",
                self.reinit_attempts
            );

            let config = match self.config.clone() {
                Some(c) => c,
                None => return Err(Error::Capture("No config for reinit".into())),
            };

            // Release old resources
            if let Some(dup) = self.duplication.take() {
                unsafe {
                    let _ = dup.ReleaseFrame();
                }
            }
            self.duplication = None;
            self.context = None;
            self.device = None;
            self.staging_texture = None;
            self.staging_dims = (0, 0);

            // Reinitialize
            self.start_capture(config).await?;
            info!("DXGI reinitialization successful");
            // Return None to signal the caller to try again on the next poll.
            return Ok(None);
        }

        // Frame rate limiting: skip if not enough time has elapsed.
        if let (Some(interval), Some(last)) = (self.frame_interval, self.last_frame_time) {
            let elapsed = last.elapsed();
            if elapsed < interval {
                return Ok(None);
            }
        }

        // Clone the duplication COM pointer so we don't hold an immutable borrow
        // of `self` across the mutable borrow needed by get_staging_texture().
        let duplication = match self.duplication.as_ref() {
            Some(d) => d.clone(),
            None => return Err(Error::Capture("Capture not started".into())),
        };

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_resource: Option<IDXGIResource> = None;

        // Attempt to acquire the next frame with a timeout.
        let acquire_result = unsafe {
            duplication.AcquireNextFrame(
                100, // 100ms timeout
                &mut frame_info,
                &mut desktop_resource,
            )
        };

        match acquire_result {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                // No new frame available yet.
                return Ok(None);
            }
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                // The desktop duplication was invalidated (e.g. mode change).
                warn!("DXGI access lost, marking for reinitialization");
                self.needs_reinit = true;
                return Err(Error::Capture(
                    "Desktop duplication access lost, needs reinitialization".into(),
                ));
            }
            Err(e) => {
                return Err(win_err(e, "AcquireNextFrame failed"));
            }
        }

        let resource = match desktop_resource {
            Some(r) => r,
            None => return Ok(None),
        };

        // Get the ID3D11Texture2D from the resource.
        let texture2d: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D = match resource.cast()
        {
            Ok(t) => t,
            Err(e) => {
                unsafe {
                    let _ = duplication.ReleaseFrame();
                }
                return Err(win_err(e, "Failed to cast resource to ID3D11Texture2D"));
            }
        };

        // Get the texture description to know dimensions and format.
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe {
            texture2d.GetDesc(&mut desc);
        }

        let width = desc.Width;
        let height = desc.Height;

        // Obtain a staging texture (cached when dimensions match).
        // Clone the device COM pointer to avoid borrowing self across get_staging_texture().
        let device = match self.device.as_ref() {
            Some(d) => d.clone(),
            None => {
                unsafe {
                    let _ = duplication.ReleaseFrame();
                }
                return Err(Error::Capture("No D3D11 device".into()));
            }
        };

        let staging_texture = match self.get_staging_texture(&device, width, height, desc.Format) {
            Ok(t) => t,
            Err(e) => {
                unsafe {
                    let _ = duplication.ReleaseFrame();
                }
                return Err(e);
            }
        };

        // Get the device context for copy and map operations.
        let context = match self.context.as_ref() {
            Some(ctx) => ctx,
            None => {
                unsafe {
                    let _ = duplication.ReleaseFrame();
                }
                return Err(Error::Capture("No device context".into()));
            }
        };

        // Copy the desktop texture into the staging texture.
        unsafe {
            context.CopyResource(&staging_texture, &texture2d);
        }

        // Map the staging texture to read pixel data.
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let map_result = unsafe {
            context.Map(
                &staging_texture,
                0, // subresource index
                D3D11_MAP_READ,
                0, // flags
                Some(&mut mapped),
            )
        };

        if let Err(e) = map_result {
            unsafe {
                let _ = duplication.ReleaseFrame();
            }
            return Err(win_err(e, "Map staging texture failed"));
        }

        // Copy BGRA pixel data from the mapped texture.
        let row_pitch = mapped.RowPitch as usize;
        let bpp = 4; // bytes per pixel for BGRA
        let image_size = (width as usize) * (height as usize) * bpp;

        let src_ptr = mapped.pData as *const u8;
        let mut pixels = Vec::with_capacity(image_size);

        for y in 0..height as usize {
            let src_offset = y * row_pitch;
            let row_len = (width as usize) * bpp;

            unsafe {
                let src = std::slice::from_raw_parts(src_ptr.add(src_offset), row_len);
                pixels.extend_from_slice(src);
            }
        }

        // Unmap the staging texture.
        unsafe {
            context.Unmap(&staging_texture, 0);
        }

        // Release the acquired frame.
        unsafe {
            let _ = duplication.ReleaseFrame();
        }

        // Compute the timestamp.
        let timestamp = self
            .start_instant
            .map(|i| i.elapsed())
            .unwrap_or(Duration::ZERO);

        // Extract dirty rects from DXGI frame info.
        let damage_regions = if frame_info.TotalMetadataBufferSize > 0 {
            extract_dirty_rects(&duplication, frame_info.TotalMetadataBufferSize as usize)
        } else {
            Vec::new()
        };

        // Extract cursor info if show_cursor is enabled.
        let show_cursor = self.config.as_ref().map(|c| c.show_cursor).unwrap_or(true);
        let cursor = extract_cursor_info(&duplication, &frame_info, show_cursor);

        // Update rate limiting state.
        self.last_frame_time = Some(Instant::now());
        self.reinit_attempts = 0;

        let monitor_id = self.config.as_ref().map(|c| c.monitor_id).unwrap_or(0);

        Ok(Some(CapturedFrame {
            data: FrameData::Cpu(pixels),
            width,
            height,
            format: PixelFormat::Bgra8888,
            damage_regions,
            timestamp,
            monitor_id,
            cursor,
        }))
    }
}
