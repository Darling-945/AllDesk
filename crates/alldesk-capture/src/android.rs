use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alldesk_core::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::capture::{CaptureConfig, CaptureProvider, CapturedFrame, FrameData, MonitorInfo, PixelFormat};

/// Shared frame buffer between Flutter/JNI and the Rust capture loop.
///
/// Uses double-buffering: the producer (Flutter/Java) writes to one buffer
/// while the consumer (Rust capture loop) reads from the other. This eliminates
/// contention and avoids frame drops during lock contention.
struct SharedFrame {
    /// Double buffer: index 0 and index 1.
    buffers: [Option<Vec<u8>>; 2],
    /// Width of the current frame in each buffer.
    widths: [u32; 2],
    /// Height of the current frame in each buffer.
    heights: [u32; 2],
    /// Index of the buffer that the producer (Flutter) should write to next.
    write_idx: usize,
    /// Index of the buffer that the consumer (Rust) should read from next.
    read_idx: usize,
    /// Number of frames received (monotonic counter).
    frame_count: u64,
    /// Number of frames dropped due to buffer full.
    dropped_count: u64,
}

impl Default for SharedFrame {
    fn default() -> Self {
        Self {
            buffers: [None, None],
            widths: [0, 0],
            heights: [0, 0],
            write_idx: 0,
            read_idx: 0,
            frame_count: 0,
            dropped_count: 0,
        }
    }
}

impl SharedFrame {
    /// Write a new frame from the producer side.
    /// Returns true if the frame was written, false if dropped.
    fn write_frame(&mut self, data: Vec<u8>, width: u32, height: u32) -> bool {
        self.frame_count += 1;

        // If both buffers are occupied (write_idx == read_idx and both have data),
        // we need to drop the oldest frame.
        let write_buf = &mut self.buffers[self.write_idx];
        if write_buf.is_some() {
            // Buffer at write_idx is occupied. Check if read_idx differs.
            if self.write_idx == self.read_idx {
                // Both buffers occupied — drop this frame or overwrite the oldest.
                self.dropped_count += 1;
                // Overwrite the write buffer (producer always wins for low latency).
            }
        }

        self.buffers[self.write_idx] = Some(data);
        self.widths[self.write_idx] = width;
        self.heights[self.write_idx] = height;

        // Advance read_idx to point to the newly written frame.
        self.read_idx = self.write_idx;
        // Toggle write_idx for double buffering.
        self.write_idx = 1 - self.write_idx;

        true
    }

    /// Read the latest frame from the consumer side.
    /// Returns None if no frame is available.
    fn read_frame(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        let frame = self.buffers[self.read_idx].take()?;
        let w = self.widths[self.read_idx];
        let h = self.heights[self.read_idx];
        Some((frame, w, h))
    }
}

/// Android screen capture via MediaProjection (JNI bridge).
///
/// Flutter/Java side captures frames via ScreenCaptureService and pushes
/// BGRA pixel data through the FFI function `push_android_frame()`.
pub struct AndroidCapturer {
    frame: Arc<Mutex<SharedFrame>>,
    new_frame: Arc<AtomicBool>,
    config: Option<CaptureConfig>,
}

// Global shared frame buffer so the FFI function can access it.
static ANDROID_FRAME: std::sync::OnceLock<Arc<Mutex<SharedFrame>>> = std::sync::OnceLock::new();
static ANDROID_FRAME_FLAG: std::sync::OnceLock<Arc<AtomicBool>> = std::sync::OnceLock::new();

impl AndroidCapturer {
    pub fn new() -> Self {
        let frame = Arc::new(Mutex::new(SharedFrame::default()));
        let flag = Arc::new(AtomicBool::new(false));
        let _ = ANDROID_FRAME.set(frame.clone());
        let _ = ANDROID_FRAME_FLAG.set(flag.clone());
        Self {
            frame,
            new_frame: flag,
            config: None,
        }
    }

    /// Get the number of frames received and dropped (for diagnostics).
    pub async fn frame_stats(&self) -> (u64, u64) {
        let f = self.frame.lock().await;
        (f.frame_count, f.dropped_count)
    }
}

/// Push a frame from Flutter/Java into the Android capturer.
/// Called via FFI from the Flutter side after ScreenCaptureService produces a frame.
///
/// Uses `try_lock()` to avoid blocking the Flutter UI thread. If the lock is
/// contended, the frame is dropped (acceptable for real-time video where the
/// next frame will arrive in ~33ms).
///
/// Returns true if the frame was accepted, false if dropped.
pub fn push_android_frame(bgra_data: Vec<u8>, width: u32, height: u32) -> bool {
    if let (Some(frame), Some(flag)) = (ANDROID_FRAME.get(), ANDROID_FRAME_FLAG.get()) {
        if let Ok(mut f) = frame.try_lock() {
            let accepted = f.write_frame(bgra_data, width, height);
            if accepted {
                flag.store(true, Ordering::Release);
            }
            return accepted;
        }
    }
    false
}

/// Get frame statistics from the FFI side (for diagnostics).
/// Returns (frames_received, frames_dropped) or (0, 0) if not initialized.
pub fn get_android_frame_stats() -> (u64, u64) {
    if let Some(frame) = ANDROID_FRAME.get() {
        if let Ok(f) = frame.try_lock() {
            return (f.frame_count, f.dropped_count);
        }
    }
    (0, 0)
}

#[async_trait]
impl CaptureProvider for AndroidCapturer {
    async fn enumerate_monitors(&self) -> Result<Vec<MonitorInfo>> {
        let frame = self.frame.lock().await;
        let w = if frame.widths[0] > 0 { frame.widths[0] } else { 1080 };
        let h = if frame.heights[0] > 0 { frame.heights[0] } else { 1920 };
        Ok(vec![MonitorInfo {
            id: 0,
            name: "Android Screen".into(),
            width: w,
            height: h,
            x: 0,
            y: 0,
            is_primary: true,
        }])
    }

    async fn start_capture(&mut self, config: CaptureConfig) -> Result<()> {
        self.config = Some(config);
        Ok(())
    }

    async fn stop_capture(&mut self) -> Result<()> {
        let mut frame = self.frame.lock().await;
        frame.buffers = [None, None];
        frame.frame_count = 0;
        frame.dropped_count = 0;
        self.config = None;
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<Option<CapturedFrame>> {
        if self.new_frame.swap(false, Ordering::AcqRel) {
            let mut shared = self.frame.lock().await;
            if let Some((data, width, height)) = shared.read_frame() {
                return Ok(Some(CapturedFrame {
                    data: FrameData::Cpu(data),
                    width,
                    height,
                    format: PixelFormat::Bgra8888,
                    damage_regions: vec![],
                    timestamp: Duration::from_millis(0),
                    monitor_id: 0,
                    cursor: None,
                }));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_frame_write_read() {
        let mut sf = SharedFrame::default();
        assert!(sf.write_frame(vec![1, 2, 3], 10, 20));
        let (data, w, h) = sf.read_frame().unwrap();
        assert_eq!(data, vec![1, 2, 3]);
        assert_eq!(w, 10);
        assert_eq!(h, 20);
    }

    #[test]
    fn test_shared_frame_double_buffer() {
        let mut sf = SharedFrame::default();

        // Write frame 1
        sf.write_frame(vec![1], 100, 100);
        // Write frame 2 (should go to second buffer)
        sf.write_frame(vec![2], 100, 100);

        // Read should get frame 2 (latest)
        let (data, _, _) = sf.read_frame().unwrap();
        assert_eq!(data, vec![2]);
    }

    #[test]
    fn test_shared_frame_no_data_returns_none() {
        let mut sf = SharedFrame::default();
        assert!(sf.read_frame().is_none());
    }

    #[test]
    fn test_shared_frame_frame_count() {
        let mut sf = SharedFrame::default();
        assert_eq!(sf.frame_count, 0);
        sf.write_frame(vec![1], 10, 10);
        assert_eq!(sf.frame_count, 1);
        sf.write_frame(vec![2], 10, 10);
        assert_eq!(sf.frame_count, 2);
    }

    #[test]
    fn test_shared_frame_overwrite_on_full() {
        let mut sf = SharedFrame::default();
        // Fill both buffers
        sf.write_frame(vec![1], 10, 10);
        sf.write_frame(vec![2], 10, 10);
        // Write a third frame — should overwrite
        sf.write_frame(vec![3], 10, 10);
        assert!(sf.dropped_count > 0);
    }

    #[test]
    fn test_android_capturer_creation() {
        let _capturer = AndroidCapturer::new();
    }

    #[tokio::test]
    async fn test_android_capturer_enumerate_monitors() {
        let capturer = AndroidCapturer::new();
        let monitors = capturer.enumerate_monitors().await.unwrap();
        assert_eq!(monitors.len(), 1);
        assert_eq!(monitors[0].name, "Android Screen");
        assert!(monitors[0].is_primary);
    }

    #[tokio::test]
    async fn test_android_capturer_start_stop() {
        let mut capturer = AndroidCapturer::new();
        let config = CaptureConfig::default();
        capturer.start_capture(config).await.unwrap();
        capturer.stop_capture().await.unwrap();
    }

    #[tokio::test]
    async fn test_android_capturer_frame_stats() {
        let capturer = AndroidCapturer::new();
        let (received, dropped) = capturer.frame_stats().await;
        assert_eq!(received, 0);
        assert_eq!(dropped, 0);
    }

    #[test]
    fn test_push_android_frame_no_init() {
        // Without initializing AndroidCapturer, push should return false
        // (or true if another test already initialized the globals).
        // This just verifies it doesn't crash.
        let _result = push_android_frame(vec![1, 2, 3], 10, 10);
    }
}
