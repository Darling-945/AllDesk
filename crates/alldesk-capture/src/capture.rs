use std::time::Duration;

use alldesk_core::Result;

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub is_primary: bool,
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub monitor_id: u32,
    pub fps: u32,
    pub show_cursor: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            monitor_id: 0,
            fps: 30,
            show_cursor: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8888,
    Nv12,
    Rgba8888,
}

pub enum FrameData {
    Cpu(Vec<u8>),
    GpuTexture(GpuTextureHandle),
}

pub struct GpuTextureHandle {
    pub handle: u64,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
}

/// Captured cursor information from the desktop.
#[derive(Debug, Clone)]
pub struct CursorInfo {
    /// Cursor X position in desktop coordinates.
    pub x: i32,
    /// Cursor Y position in desktop coordinates.
    pub y: i32,
    /// Whether the cursor is visible.
    pub visible: bool,
    /// Cursor shape image data (BGRA pixels for monochrome/color, or alpha mask).
    pub shape_data: Option<Vec<u8>>,
    /// Width of the cursor shape image.
    pub shape_width: u32,
    /// Height of the cursor shape image.
    pub shape_height: u32,
    /// Hot spot X offset within the cursor shape.
    pub hot_spot_x: u32,
    /// Hot spot Y offset within the cursor shape.
    pub hot_spot_y: u32,
    /// Type of cursor shape.
    pub shape_type: CursorShapeType,
}

/// Type of cursor shape data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShapeType {
    /// Monochrome cursor: shape_data is 1bpp mask.
    Monochrome,
    /// Color cursor: shape_data is BGRA pixels.
    Color,
    /// Masked color cursor: BGRA pixels with alpha mask.
    MaskedColor,
}

pub struct CapturedFrame {
    pub data: FrameData,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub damage_regions: Vec<Rect>,
    pub timestamp: Duration,
    pub monitor_id: u32,
    /// Cursor information if show_cursor is enabled and cursor data is available.
    pub cursor: Option<CursorInfo>,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[async_trait::async_trait]
pub trait CaptureProvider: Send + Sync {
    async fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>>;
    async fn start_capture(&mut self, config: CaptureConfig) -> Result<()>;
    async fn stop_capture(&mut self) -> Result<()>;
    async fn next_frame(&mut self) -> Result<Option<CapturedFrame>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_config_default() {
        let config = CaptureConfig::default();
        assert_eq!(config.monitor_id, 0);
        assert_eq!(config.fps, 30);
        assert!(config.show_cursor);
    }

    #[test]
    fn test_pixel_format_variants() {
        assert_ne!(PixelFormat::Bgra8888, PixelFormat::Rgba8888);
        assert_ne!(PixelFormat::Bgra8888, PixelFormat::Nv12);
    }

    #[test]
    fn test_rect_fields() {
        let rect = Rect { x: 10, y: 20, width: 100, height: 200 };
        assert_eq!(rect.x, 10);
        assert_eq!(rect.width, 100);
    }

    #[test]
    fn test_monitor_info_fields() {
        let info = MonitorInfo {
            id: 0,
            name: "Primary".to_string(),
            width: 1920,
            height: 1080,
            x: 0,
            y: 0,
            is_primary: true,
        };
        assert!(info.is_primary);
        assert_eq!(info.width * info.height, 1920 * 1080);
    }

    #[test]
    fn test_cursor_info_fields() {
        let cursor = CursorInfo {
            x: 100,
            y: 200,
            visible: true,
            shape_data: Some(vec![0u8; 64]),
            shape_width: 16,
            shape_height: 16,
            hot_spot_x: 0,
            hot_spot_y: 0,
            shape_type: CursorShapeType::Color,
        };
        assert!(cursor.visible);
        assert_eq!(cursor.x, 100);
        assert_eq!(cursor.shape_width, 16);
        assert!(cursor.shape_data.is_some());
    }

    #[test]
    fn test_cursor_shape_type_variants() {
        assert_ne!(CursorShapeType::Monochrome, CursorShapeType::Color);
        assert_ne!(CursorShapeType::Color, CursorShapeType::MaskedColor);
    }
}
