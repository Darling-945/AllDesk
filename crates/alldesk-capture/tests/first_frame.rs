//! Live capture diagnostics: does DxgiCapturer deliver a first frame on a
//! quiet desktop? Guard for the "connected but no video" class of bugs.

use alldesk_capture::capture::{CaptureConfig, CaptureProvider};

#[cfg(target_os = "windows")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dxgi_delivers_first_frame_on_quiet_desktop() {
    use alldesk_capture::dxgi::DxgiCapturer;
    let mut capturer = DxgiCapturer::new();
    let monitors = capturer.enumerate_monitors().await.expect("enumerate");
    if monitors.is_empty() {
        eprintln!("SKIPPED: no monitors");
        return;
    }
    let mon = &monitors[0];
    capturer
        .start_capture(CaptureConfig {
            monitor_id: mon.id,
            fps: 30,
            show_cursor: true,
        })
        .await
        .expect("start capture");

    // Poll next_frame for up to 10 s. AcquireNextFrame on a quiet desktop
    // never returns the current image; the capturer bridges that with a
    // one-shot GDI fallback, so a frame must arrive immediately.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut timeouts = 0u32;
    while std::time::Instant::now() < deadline {
        match capturer.next_frame().await {
            Ok(Some(frame)) => {
                eprintln!(
                    "first frame: {}x{} after {} timeouts",
                    frame.width, frame.height, timeouts
                );
                assert!(frame.width > 0 && frame.height > 0);
                return;
            }
            Ok(None) => timeouts += 1,
            Err(e) => panic!("capture error: {e}"),
        }
    }
    panic!("no first frame within 10s on a quiet desktop ({timeouts} timeouts)");
}
