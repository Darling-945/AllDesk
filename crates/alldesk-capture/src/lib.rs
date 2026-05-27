pub mod capture;

cfg_if::cfg_if! {
    if #[cfg(target_os = "windows")] {
        pub mod dxgi;
        pub use dxgi::DxgiCapturer;
    } else if #[cfg(target_os = "android")] {
        pub mod android;
        pub use android::AndroidCapturer;
    } else if #[cfg(target_os = "macos")] {
        // pub mod quartz;
    } else if #[cfg(target_os = "linux")] {
        // pub mod x11;
        // pub mod wayland;
    }
}

pub use capture::{CaptureProvider, CapturedFrame, CursorInfo, CursorShapeType, FrameData, MonitorInfo, PixelFormat, CaptureConfig};
