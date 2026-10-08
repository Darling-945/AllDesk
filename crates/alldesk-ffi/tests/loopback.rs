//! End-to-end loopback test: real QUIC connection, real DXGI capture, real
//! VP9 encode/decode — the same SenderPipeline/ReceiverPipeline the app
//! runs. Guards against regressions in the "connected but no video" class
//! of bugs (stream routing, pacing, framing, decode gating).
//!
//! Also drives the real file-transfer wire protocol (send_file_session →
//! run_file_receiver) over a real QUIC connection.
//!
//! Soft-skips when the environment has no capturable monitor (headless CI).

#[cfg(target_os = "windows")]
use alldesk_ffi::pipeline::{ReceiverPipeline, SenderPipeline};
use alldesk_net::QuicEndpoint;
use std::time::Duration;

#[cfg(target_os = "windows")]
// exercises the DXGI capture path; running it
// on macOS would pop the Screen Recording TCC prompt during `cargo test`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn video_loopback_delivers_frames() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,alldesk_ffi=debug,alldesk_net=debug".into()),
        )
        .try_init();

    // --- Host side: QUIC server endpoint on an ephemeral port ---
    let server = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).expect("server endpoint");
    let addr = server.local_addr().expect("local addr");

    // --- Viewer side: connect (the server must have a pending accept for
    // the handshake to complete in this quinn version, same as the app's
    // always-accepting host loop) ---
    let accepted = {
        let server = server.clone();
        tokio::spawn(async move { server.accept().await })
    };

    let client = QuicEndpoint::new_client().expect("client endpoint");
    let conn = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.connect(addr).await {
                Ok(conn) => return conn,
                Err(e) => {
                    eprintln!("connect attempt failed: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    })
    .await
    .expect("connect within 5s");

    // Viewer pipeline first, so the receiver is subscribing before frames flow.
    let viewer_transport = alldesk_net::transport::QuicTransport::new(conn.clone(), true);
    let (mut rx_pipeline, frame_tx) = ReceiverPipeline::new(viewer_transport, 1920, 1080);
    let mut frame_rx = frame_tx.subscribe();
    tokio::spawn(async move {
        if let Err(e) = rx_pipeline.run().await {
            eprintln!("receiver pipeline ended: {e}");
        }
    });

    // Accept finished while connecting; run the sender pipeline.
    let accepted = accepted
        .await
        .expect("accept task alive")
        .expect("accepted connection");

    let host_transport = alldesk_net::transport::QuicTransport::new(accepted, true);
    let mut sender = match SenderPipeline::new(host_transport, 4000, 30).await {
        Ok(s) => s,
        Err(e) => {
            // Headless/locked session has no capturable output — skip.
            eprintln!("SKIPPED: capture unavailable: {e}");
            return;
        }
    };
    tokio::spawn(async move {
        if let Err(e) = sender.run().await {
            eprintln!("sender pipeline ended: {e}");
        }
    });

    // A real frame must arrive within a generous window (DXGI init + first
    // AcquireNextFrame + handshake + decode).
    let deadline = Duration::from_secs(20);
    let frame = tokio::time::timeout(deadline, frame_rx.recv()).await;
    match frame {
        Ok(Ok(f)) => {
            assert!(
                f.width > 0 && f.height > 0,
                "frame dims: {}x{}",
                f.width,
                f.height
            );
            assert_eq!(
                f.bgra_data.len(),
                f.width as usize * f.height as usize * 4,
                "BGRA payload must match dims exactly (display gate)"
            );
        }
        Ok(Err(e)) => panic!("frame channel closed before a frame: {e}"),
        Err(_) => panic!("no video frame within {deadline:?} — connected but no video"),
    }
}

/// Real QUIC connection + the real file-transfer wire protocol: the viewer
/// side streams a temp file with `send_file_session`, the host side
/// `run_file_receiver` writes it into `ALLDESK_RECEIVED_FILES_DIR`. The
/// received bytes must match the sent bytes exactly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_transfer_end_to_end() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,alldesk_ffi=debug,alldesk_net=debug".into()),
        )
        .try_init();

    // Unique dirs so parallel test runs cannot collide.
    let tag = format!(
        "alldesk-filetest-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let work_dir = std::env::temp_dir().join(&tag);
    let recv_dir = work_dir.join("received");
    std::fs::create_dir_all(&recv_dir).expect("create receive dir");

    // ~700 KB with a non-trivial, deterministic pattern (spans many chunks).
    let mut sent = Vec::with_capacity(700_000);
    let mut x: u32 = 0x1234_5678;
    for _ in 0..175_000 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        sent.extend_from_slice(&x.to_le_bytes());
    }
    let src_path = work_dir.join("payload.bin");
    std::fs::write(&src_path, &sent).expect("write source file");

    // Route the host-side receiver into the temp dir for this test.
    std::env::set_var("ALLDESK_RECEIVED_FILES_DIR", &recv_dir);

    let server = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).expect("server endpoint");
    let addr = server.local_addr().expect("local addr");
    let accepted = {
        let server = server.clone();
        tokio::spawn(async move { server.accept().await })
    };

    let client = QuicEndpoint::new_client().expect("client endpoint");
    let conn = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match client.connect(addr).await {
                Ok(conn) => return conn,
                Err(e) => {
                    eprintln!("connect attempt failed: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    })
    .await
    .expect("connect within 5s");

    let accepted = accepted
        .await
        .expect("accept task alive")
        .expect("accepted connection");

    // Host: receiver loop first, so the File channel is being accepted.
    let host_transport = alldesk_net::transport::QuicTransport::new(accepted, true);
    tokio::spawn(async move {
        alldesk_ffi::run_file_receiver(host_transport).await;
    });

    // Viewer: stream the file through the same code path the app uses.
    let viewer_transport = alldesk_net::transport::QuicTransport::new(conn, true);
    let total = sent.len() as u64;
    alldesk_ffi::send_file_session(
        viewer_transport,
        src_path.to_str().unwrap(),
        "payload.bin",
        total,
    )
    .await
    .expect("send file session");

    // The END message flushes and closes the file; poll for completion.
    let dest = recv_dir.join("payload.bin");
    let received = tokio::time::timeout(Duration::from_secs(10), async {
        let mut ticks = 0u32;
        loop {
            match std::fs::read(&dest) {
                Ok(bytes) if bytes.len() as u64 == total => return bytes,
                Ok(bytes) => {
                    if ticks % 20 == 0 {
                        eprintln!("poll: partial file, {} / {} bytes", bytes.len(), total);
                    }
                }
                Err(e) => {
                    if ticks % 20 == 0 {
                        eprintln!("poll: read {dest:?}: {e}");
                    }
                }
            }
            ticks += 1;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("file received within 10s");

    assert_eq!(received, sent, "received bytes must match sent bytes");

    std::env::remove_var("ALLDESK_RECEIVED_FILES_DIR");
    let _ = std::fs::remove_dir_all(&work_dir);
}
