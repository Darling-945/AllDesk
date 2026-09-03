use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};

use crate::connection_quality::{ConnectionQuality, QualityCollector};
use crate::pipeline::{
    AudioReceiverPipeline, AudioSenderPipeline, ClipboardPipeline, ReceiverPipeline,
    SenderPipeline, VideoFrame,
};
use alldesk_core::adaptive::{AdaptiveController, AdaptiveTargets, LossRateTracker};
use alldesk_core::config::AppConfig;
use alldesk_net::channel::Channel;
use alldesk_net::reconnect::ReconnectManager;
use alldesk_net::{LanDiscovery, QuicEndpoint, Transport};

use alldesk_files::transfer::FileTransfer;
use alldesk_platform::input::{ButtonState, InputController, KeyCode, KeyState, MouseButton};
use alldesk_recording::Recorder;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(500);
/// Max automatic reconnect attempts after a connection drop.
const MAX_RECONNECT_ATTEMPTS: u32 = 5;

/// Adaptive encoding bounds: the control loop moves the encoder bitrate and
/// capture FPS between these based on observed RTT/loss.
const ADAPTIVE_INITIAL_BITRATE_KBPS: u32 = 4000;
const ADAPTIVE_MIN_BITRATE_KBPS: u32 = 500;
const ADAPTIVE_MAX_BITRATE_KBPS: u32 = 8000;
const ADAPTIVE_MIN_FPS: u32 = 5;
const ADAPTIVE_MAX_FPS: u32 = 30;

/// Get the library version
pub fn get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// A discovered peer on the LAN
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub peer_name: String,
    pub address: String,
}

/// Session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    Failed,
}

// ---------- Cached peer ID ----------

static PEER_ID: OnceLock<String> = OnceLock::new();

fn ensure_peer_id() -> &'static str {
    PEER_ID.get_or_init(|| AppConfig::default().peer_id)
}

// ---------- Shared runtime state ----------

struct ServerState {
    endpoint: QuicEndpoint,
    port: u16,
}

struct ClientState {
    endpoint: QuicEndpoint,
    /// Kept for diagnostics/future use; the supervisor holds its own clone.
    #[allow(dead_code)]
    conn: quinn::Connection,
    remote_addr: String,
    /// Drives automatic reconnects for this session (held by the supervisor).
    #[allow(dead_code)]
    reconnect: Option<Arc<ReconnectManager>>,
}

/// Progress of an ongoing file transfer, surfaced to Flutter as JSON.
#[derive(Debug, Clone, Serialize)]
pub struct FileTransferProgress {
    pub active: bool,
    pub direction: String,
    pub filename: String,
    pub transferred: u64,
    pub total: u64,
    pub error: Option<String>,
}

impl Default for FileTransferProgress {
    fn default() -> Self {
        Self {
            active: false,
            direction: "none".into(),
            filename: String::new(),
            transferred: 0,
            total: 0,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedPeer {
    peer_id: String,
    peer_name: String,
    address: String,
}

static APP_STATE: OnceLock<RwLock<AppState>> = OnceLock::new();

// Separate fine-grained locks to avoid contention between frame polling and input sending.
static FRAME_RX: OnceLock<Mutex<Option<tokio::sync::broadcast::Receiver<VideoFrame>>>> =
    OnceLock::new();
static INPUT_TRANSPORT: OnceLock<Mutex<Option<alldesk_net::transport::QuicTransport>>> =
    OnceLock::new();
/// Viewer-side session recorder (set by start_session_recording).
static RECORDER: OnceLock<Mutex<Option<Arc<std::sync::Mutex<Recorder>>>>> = OnceLock::new();

fn frame_rx_lock() -> &'static Mutex<Option<tokio::sync::broadcast::Receiver<VideoFrame>>> {
    FRAME_RX.get_or_init(|| Mutex::new(None))
}

fn input_transport_lock() -> &'static Mutex<Option<alldesk_net::transport::QuicTransport>> {
    INPUT_TRANSPORT.get_or_init(|| Mutex::new(None))
}

fn recorder_lock() -> &'static Mutex<Option<Arc<std::sync::Mutex<Recorder>>>> {
    RECORDER.get_or_init(|| Mutex::new(None))
}

struct AppState {
    server: Option<ServerState>,
    client: Option<ClientState>,
    sender_task: Option<tokio::task::JoinHandle<()>>,
    receiver_task: Option<tokio::task::JoinHandle<()>>,
    input_task: Option<tokio::task::JoinHandle<()>>,
    audio_sender_task: Option<tokio::task::JoinHandle<()>>,
    audio_receiver_task: Option<tokio::task::JoinHandle<()>>,
    clipboard_task: Option<tokio::task::JoinHandle<()>>,
    /// Watches the viewer connection and rebuilds the session after drops.
    reconnect_task: Option<tokio::task::JoinHandle<()>>,
    /// Samples RTT/loss/bandwidth while a viewer session is active.
    quality_task: Option<tokio::task::JoinHandle<()>>,
    /// Bumped on every install_viewer_session; lets stale supervisors exit.
    session_generation: u64,
    discovered_peers: Vec<CachedPeer>,
    /// Tracks whether video frames are flowing (viewer side).
    video_active: bool,
    /// Last error from sender/receiver pipeline.
    last_pipeline_error: Option<String>,
    /// Number of video frames received (viewer side).
    frames_received: u64,
    /// Connection quality samples fed by the per-session sampler task.
    quality: Arc<std::sync::Mutex<QualityCollector>>,
    /// Ongoing file transfer, if any (viewer sending or host receiving).
    file_progress: FileTransferProgress,
}

fn app_state() -> &'static RwLock<AppState> {
    APP_STATE.get_or_init(|| {
        RwLock::new(AppState {
            server: None,
            client: None,
            sender_task: None,
            receiver_task: None,
            input_task: None,
            audio_sender_task: None,
            audio_receiver_task: None,
            clipboard_task: None,
            reconnect_task: None,
            quality_task: None,
            session_generation: 0,
            discovered_peers: Vec::new(),
            video_active: false,
            last_pipeline_error: None,
            frames_received: 0,
            quality: Arc::new(std::sync::Mutex::new(QualityCollector::new())),
            file_progress: FileTransferProgress::default(),
        })
    })
}

// ---------- FFI API ----------

/// Initialize the AllDesk core library. Call once at app startup.
/// Auto-starts QUIC server + background discovery broadcast/listen.
pub async fn init() -> String {
    let _ = ensure_peer_id();

    // Initialize logging (ignore error if already initialized)
    let _ = tracing_subscriber::fmt::try_init();

    // Auto-start QUIC server and wait for it to be ready
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        match start_server_internal(21116, Some(ready_tx)).await {
            Ok(msg) => tracing::info!("{}", msg),
            Err(e) => tracing::warn!("auto-start server: {}", e),
        }
    });

    // Wait up to 3s for the server to start listening
    match tokio::time::timeout(Duration::from_secs(3), ready_rx).await {
        Ok(Ok(())) => tracing::info!("server ready"),
        Ok(Err(_)) => tracing::warn!("server start signal dropped"),
        Err(_) => tracing::warn!("server start timed out after 3s"),
    }

    // Start background discovery: broadcast our presence + listen for others
    let peer_id = ensure_peer_id().to_string();
    let peer_name = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "Unknown".into());

    tracing::info!("starting discovery as '{}' ({})", peer_name, peer_id);

    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<alldesk_net::PeerDiscovered>(32);

        tokio::spawn(async move {
            if let Err(e) = LanDiscovery::discover_loop(peer_id, peer_name, 21116, tx).await {
                tracing::error!("discovery loop error: {}", e);
            }
        });

        while let Some(peer) = rx.recv().await {
            let cached = CachedPeer {
                peer_id: peer.peer_id.clone(),
                peer_name: peer.peer_name.clone(),
                address: peer.addr.to_string(),
            };
            let mut state = app_state().write().await;
            if let Some(existing) = state
                .discovered_peers
                .iter_mut()
                .find(|p| p.peer_id == cached.peer_id)
            {
                *existing = cached;
            } else {
                tracing::info!("discovered peer: {} @ {}", cached.peer_name, cached.address);
                state.discovered_peers.push(cached);
            }
        }
    });

    "initialized".to_string()
}

/// Return currently known peers from the background discovery cache.
/// When timeout_secs > 0, also performs an active broadcast scan.
pub async fn discover_peers(timeout_secs: u64) -> Vec<PeerInfo> {
    // Active scan on explicit request
    if timeout_secs > 0 {
        let discovery = LanDiscovery::new(21116);
        match tokio::time::timeout(Duration::from_secs(timeout_secs), discovery.discover()).await {
            Ok(Ok(peers)) => {
                let mut state = app_state().write().await;
                for peer in peers {
                    let cached = CachedPeer {
                        peer_id: peer.peer_id.clone(),
                        peer_name: peer.peer_name.clone(),
                        address: peer.addr.to_string(),
                    };
                    if let Some(existing) = state
                        .discovered_peers
                        .iter_mut()
                        .find(|p| p.peer_id == cached.peer_id)
                    {
                        *existing = cached;
                    } else {
                        state.discovered_peers.push(cached);
                    }
                }
            }
            Ok(Err(e)) => tracing::warn!("active discover error: {}", e),
            Err(_) => tracing::warn!("active discover timed out"),
        }
    }

    let state = app_state().read().await;
    state
        .discovered_peers
        .iter()
        .map(|p| PeerInfo {
            peer_id: p.peer_id.clone(),
            peer_name: p.peer_name.clone(),
            address: p.address.clone(),
        })
        .collect()
}

async fn start_server_internal(
    port: u16,
    ready_signal: Option<tokio::sync::oneshot::Sender<()>>,
) -> Result<String, String> {
    let bind_addr: SocketAddr = format!("0.0.0.0:{}", port)
        .parse()
        .map_err(|e| format!("invalid port {}: {}", port, e))?;

    let endpoint = QuicEndpoint::new_server(bind_addr)
        .map_err(|e| format!("failed to create server: {}", e))?;

    let local = endpoint
        .local_addr()
        .map_err(|e| format!("failed to get local addr: {}", e))?;

    // Signal that the server is now listening
    if let Some(tx) = ready_signal {
        let _ = tx.send(());
    }

    // Spawn auto-accept loop: when a viewer connects, start streaming + input handler
    let accept_endpoint = endpoint.clone();
    let accept_handle = tokio::spawn(async move {
        loop {
            match accept_endpoint.accept().await {
                Ok(conn) => {
                    tracing::info!("viewer connected from {}", conn.remote_address());

                    // All pipelines on this connection share one stream table
                    // so their accept_channel() calls don't steal each
                    // other's streams.
                    let video_transport =
                        alldesk_net::transport::QuicTransport::new(conn.clone(), true);
                    let shared_streams = video_transport.shared_streams();
                    let audio_host_transport =
                        alldesk_net::transport::QuicTransport::with_shared_streams(
                            conn.clone(),
                            true,
                            shared_streams.clone(),
                        );
                    let clipboard_host_transport =
                        alldesk_net::transport::QuicTransport::with_shared_streams(
                            conn.clone(),
                            true,
                            shared_streams.clone(),
                        );
                    let file_host_transport =
                        alldesk_net::transport::QuicTransport::with_shared_streams(
                            conn.clone(),
                            true,
                            shared_streams.clone(),
                        );

                    // Start input handler concurrently (host-side: receive viewer input → inject to OS)
                    let input_conn = conn.clone();
                    let input_handle = tokio::spawn(run_input_handler(input_conn, shared_streams));

                    // Adaptive encoding: sample RTT/loss every second and
                    // publish new bitrate/FPS targets to the sender pipeline.
                    let (adapt_tx, adapt_rx) = tokio::sync::watch::channel(AdaptiveTargets {
                        bitrate_kbps: ADAPTIVE_INITIAL_BITRATE_KBPS,
                        fps: ADAPTIVE_MAX_FPS,
                    });
                    let adaptive_conn = conn.clone();
                    let adaptive_handle =
                        tokio::spawn(run_adaptive_controller(adaptive_conn, adapt_tx));

                    // Start sender pipeline concurrently (capture → VP9 → viewer)
                    let sender_handle = tokio::spawn(async move {
                        tracing::info!("initializing sender pipeline...");
                        match SenderPipeline::new(
                            video_transport,
                            ADAPTIVE_INITIAL_BITRATE_KBPS,
                            ADAPTIVE_MAX_FPS,
                        )
                        .await
                        {
                            Ok(pipeline) => {
                                tracing::info!("sender pipeline started, beginning capture loop");
                                let mut pipeline = pipeline.with_adaptive(adapt_rx);
                                if let Err(e) = pipeline.run().await {
                                    tracing::error!("sender pipeline error: {}", e);
                                }
                            }
                            Err(e) => {
                                tracing::error!("failed to create sender pipeline: {}", e);
                            }
                        }
                        tracing::info!("sender pipeline exited");
                    });

                    // Start audio sender (host mic → viewer speaker)
                    let audio_sender_handle = tokio::spawn(async move {
                        match AudioSenderPipeline::new(audio_host_transport) {
                            Ok(mut pipeline) => {
                                tracing::info!("audio sender pipeline started");
                                if let Err(e) = pipeline.run().await {
                                    tracing::error!("audio sender error: {}", e);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("audio sender init failed (non-fatal): {}", e);
                            }
                        }
                    });

                    // Start clipboard sync (bidirectional)
                    let clipboard_handle = tokio::spawn(async move {
                        match ClipboardPipeline::new(clipboard_host_transport, false) {
                            Ok(mut pipeline) => {
                                tracing::info!("clipboard sync pipeline started");
                                if let Err(e) = pipeline.run().await {
                                    tracing::error!("clipboard sync error: {}", e);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("clipboard sync init failed (non-fatal): {}", e);
                            }
                        }
                    });

                    // File receiver (viewer → host file push)
                    let file_handle = tokio::spawn(async move {
                        run_file_receiver(file_host_transport).await;
                    });

                    // Wait for all tasks
                    let _ = tokio::join!(
                        sender_handle,
                        input_handle,
                        audio_sender_handle,
                        clipboard_handle,
                        file_handle,
                        adaptive_handle
                    );
                    tracing::info!("viewer session ended");
                }
                Err(e) => {
                    tracing::error!("accept connection failed: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    });

    let mut state = app_state().write().await;
    if let Some(old) = state.server.take() {
        old.endpoint.close();
    }
    if let Some(h) = state.sender_task.take() {
        h.abort();
    }
    if let Some(h) = state.input_task.take() {
        h.abort();
    }
    state.server = Some(ServerState { endpoint, port });
    state.sender_task = Some(accept_handle);

    tracing::info!("server listening on {} (auto-accept enabled)", local);
    Ok(format!("server listening on {} (auto-accept)", local))
}

/// Start listening for incoming QUIC connections on the given port.
pub async fn start_server(port: u16) -> Result<String, String> {
    start_server_internal(port, None).await
}

/// Connect to a remote peer via QUIC and start the receiver pipeline.
pub async fn connect_to_peer(addr: String) -> Result<String, String> {
    let remote: SocketAddr = addr
        .parse()
        .map_err(|e| format!("invalid address '{}': {}", addr, e))?;

    let endpoint = QuicEndpoint::new_client()
        .map_err(|e| format!("failed to create client endpoint: {}", e))?;

    // Retry loop with timeout on each attempt for the INITIAL connection;
    // later drops are handled by the reconnect supervisor.
    let reconnect = Arc::new(
        ReconnectManager::new(endpoint.clone(), remote).with_max_attempts(MAX_RECONNECT_ATTEMPTS),
    );
    let mut last_err = String::new();
    for attempt in 1..=MAX_RETRIES {
        match tokio::time::timeout(CONNECT_TIMEOUT, reconnect.connect()).await {
            Ok(Ok(conn)) => {
                // Stop a supervisor left over from a previous session; we are
                // not inside it, so aborting here is safe.
                let mut state = app_state().write().await;
                if let Some(h) = state.reconnect_task.take() {
                    h.abort();
                }
                drop(state);

                install_viewer_session(
                    reconnect.clone(),
                    endpoint.clone(),
                    conn.clone(),
                    addr.clone(),
                )
                .await;

                // Watch the connection and rebuild the session after drops.
                let generation = app_state().read().await.session_generation;
                app_state().write().await.reconnect_task = Some(tokio::spawn(
                    supervise_connection(reconnect, endpoint, conn, addr.clone(), generation),
                ));
                tracing::info!("connected to {}", addr);
                return Ok(format!("connected to {}", addr));
            }
            Ok(Err(e)) => {
                last_err = format!("connection to {} failed: {}", addr, e);
                tracing::warn!("connect attempt {}/{}: {}", attempt, MAX_RETRIES, last_err);
            }
            Err(_) => {
                last_err = format!(
                    "connection to {} timed out after {:?}",
                    addr, CONNECT_TIMEOUT
                );
                tracing::warn!("connect attempt {}/{}: timed out", attempt, MAX_RETRIES);
            }
        }

        if attempt < MAX_RETRIES {
            tokio::time::sleep(RETRY_DELAY * attempt).await;
        }
    }

    Err(last_err)
}

/// Build all viewer-side pipelines on a (fresh) connection, store them in the
/// app state and start the reconnect supervisor.
async fn install_viewer_session(
    reconnect: Arc<ReconnectManager>,
    endpoint: QuicEndpoint,
    conn: quinn::Connection,
    remote_addr: String,
) {
    // All pipelines on this connection share one stream table.
    let transport = alldesk_net::transport::QuicTransport::new(conn.clone(), true);
    let shared_streams = transport.shared_streams();
    let (mut rx_pipeline, frame_rx) = ReceiverPipeline::new(transport, 1920, 1080);

    let receiver_task = tokio::spawn(async move {
        if let Err(e) = rx_pipeline.run().await {
            tracing::error!("receiver pipeline: {}", e);
            record_pipeline_error(format!("receiver: {}", e)).await;
        }
    });

    let input_transport = alldesk_net::transport::QuicTransport::with_shared_streams(
        conn.clone(),
        true,
        shared_streams.clone(),
    );

    // Audio receiver (viewer speaker ← host mic)
    let audio_viewer_transport = alldesk_net::transport::QuicTransport::with_shared_streams(
        conn.clone(),
        true,
        shared_streams.clone(),
    );
    let audio_receiver_task = tokio::spawn(async move {
        let mut pipeline = AudioReceiverPipeline::new(audio_viewer_transport);
        tracing::info!("audio receiver pipeline starting");
        if let Err(e) = pipeline.run().await {
            tracing::error!("audio receiver error: {}", e);
            record_pipeline_error(format!("audio receiver: {}", e)).await;
        }
    });

    // Clipboard sync (bidirectional, viewer side)
    let clipboard_viewer_transport = alldesk_net::transport::QuicTransport::with_shared_streams(
        conn.clone(),
        true,
        shared_streams.clone(),
    );
    let clipboard_task = tokio::spawn(async move {
        match ClipboardPipeline::new(clipboard_viewer_transport, true) {
            Ok(mut pipeline) => {
                tracing::info!("clipboard receiver pipeline starting");
                if let Err(e) = pipeline.run().await {
                    tracing::error!("clipboard sync error: {}", e);
                    record_pipeline_error(format!("clipboard: {}", e)).await;
                }
            }
            Err(e) => {
                tracing::warn!("clipboard init failed (non-fatal): {}", e);
            }
        }
    });

    // Sample RTT / loss / bandwidth from the connection once per second.
    let quality = app_state().read().await.quality.clone();
    if let Ok(mut q) = quality.lock() {
        q.reset();
    }
    let quality_task = tokio::spawn(run_quality_sampler(conn.clone(), quality));

    let mut state = app_state().write().await;
    if let Some(old) = state.client.take() {
        old.endpoint.close();
    }
    // Abort stale pipeline tasks from a previous session. The old supervisor
    // is NOT aborted here — it may be our caller; it exits on its own via the
    // generation check below.
    for h in [
        &state.receiver_task,
        &state.audio_receiver_task,
        &state.clipboard_task,
        &state.quality_task,
    ]
    .into_iter()
    .flatten()
    {
        h.abort();
    }
    state.session_generation += 1;

    state.client = Some(ClientState {
        endpoint: endpoint.clone(),
        conn: conn.clone(),
        remote_addr: remote_addr.clone(),
        reconnect: Some(reconnect.clone()),
    });
    state.receiver_task = Some(receiver_task);
    state.audio_receiver_task = Some(audio_receiver_task);
    state.clipboard_task = Some(clipboard_task);
    state.quality_task = Some(quality_task);
    state.video_active = false;
    state.last_pipeline_error = None;
    state.frames_received = 0;

    // Store frame_rx and input_transport in their own fine-grained locks
    *frame_rx_lock().lock().await = Some(frame_rx);
    *input_transport_lock().lock().await = Some(input_transport);
}

/// Automatically reconnect after a connection drop (viewer side). Runs for
/// the lifetime of the viewer role: whenever the current connection dies and
/// the user hasn't disconnected, rebuilds the whole session on a fresh one.
async fn supervise_connection(
    reconnect: Arc<ReconnectManager>,
    endpoint: QuicEndpoint,
    mut conn: quinn::Connection,
    remote_addr: String,
    mut generation: u64,
) {
    loop {
        // Wait until this connection actually dies.
        while conn.close_reason().is_none() {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Give a potential clean disconnect a moment to land.
        tokio::time::sleep(Duration::from_millis(500)).await;

        loop {
            let (still_connected, was_active, current_gen) = {
                let state = app_state().read().await;
                (
                    state.client.is_some(),
                    state.video_active,
                    state.session_generation,
                )
            };

            // User called disconnect() or a newer session took over (manual
            // reconnect from the UI) — nothing to do. Also give up if this
            // session never showed video (host likely has no capturer).
            if !still_connected || !was_active || current_gen != generation {
                return;
            }

            tracing::warn!("connection to {} lost, reconnecting...", remote_addr);
            reconnect.mark_disconnected().await;
            match reconnect.reconnect().await {
                Ok(new_conn) => {
                    tracing::info!("reconnected to {}", remote_addr);
                    install_viewer_session(
                        reconnect.clone(),
                        endpoint.clone(),
                        new_conn.clone(),
                        remote_addr.clone(),
                    )
                    .await;
                    conn = new_conn;
                    generation = app_state().read().await.session_generation;
                    break; // back to watching the new connection
                }
                Err(e) => {
                    record_pipeline_error(format!("reconnect failed: {}", e)).await;
                    if !reconnect.can_reconnect().await {
                        tracing::error!("giving up reconnecting to {}", remote_addr);
                        return;
                    }
                }
            }
        }
    }
}

/// Feed RTT / packet loss / bandwidth from quinn into the quality collector.
async fn run_quality_sampler(
    conn: quinn::Connection,
    quality: Arc<std::sync::Mutex<QualityCollector>>,
) {
    let mut last_rx_bytes: u64 = 0;
    let mut last_sample = std::time::Instant::now();
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if conn.close_reason().is_some() {
            return;
        }
        let stats = conn.stats();
        let rx_bytes = stats.udp_rx.bytes;
        let elapsed = last_sample.elapsed().as_secs_f64().max(0.001);
        let bandwidth_kbps = rx_bytes.saturating_sub(last_rx_bytes) * 8 / 1024 / elapsed as u64;
        last_rx_bytes = rx_bytes;
        last_sample = std::time::Instant::now();

        if let Ok(mut q) = quality.lock() {
            q.record_rtt(stats.path.rtt.as_secs_f64() * 1000.0);
            q.record_packet_counts(stats.path.sent_packets, stats.path.lost_packets);
            q.set_bandwidth(bandwidth_kbps);
        }
    }
}

/// Host-side adaptive control loop: once per second, read the connection's
/// RTT and per-interval packet loss and publish new encoder/pacing targets
/// to the sender pipeline. Exits when the connection closes or the pipeline
/// (the watch receiver) is gone.
async fn run_adaptive_controller(
    conn: quinn::Connection,
    tx: tokio::sync::watch::Sender<AdaptiveTargets>,
) {
    let mut controller = AdaptiveController::new(
        ADAPTIVE_INITIAL_BITRATE_KBPS,
        ADAPTIVE_MIN_BITRATE_KBPS,
        ADAPTIVE_MAX_BITRATE_KBPS,
        ADAPTIVE_MIN_FPS,
        ADAPTIVE_MAX_FPS,
    );
    let mut loss = LossRateTracker::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The channel is initialized with the same targets; only publish deltas.
    let mut last_sent = AdaptiveTargets {
        bitrate_kbps: ADAPTIVE_INITIAL_BITRATE_KBPS,
        fps: ADAPTIVE_MAX_FPS,
    };

    loop {
        tick.tick().await;
        if conn.close_reason().is_some() {
            return;
        }

        let stats = conn.stats();
        let rtt_ms = stats.path.rtt.as_secs_f64() * 1000.0;
        let loss_rate = loss.update(stats.path.sent_packets, stats.path.lost_packets);

        let targets = controller.update(rtt_ms, loss_rate);
        tracing::debug!(
            rtt_ms = rtt_ms as u64,
            loss_rate,
            bitrate_kbps = targets.bitrate_kbps,
            fps = targets.fps,
            "adaptive sample"
        );
        // Send only on change so the pipeline's has_changed() check doesn't
        // wake it with identical targets every second.
        if targets != last_sent {
            last_sent = targets;
            if tx.send(targets).is_err() {
                return; // sender pipeline is gone — session over
            }
        }
    }
}

/// Disconnect from current peer and stop server.
pub async fn disconnect() -> String {
    let mut state = app_state().write().await;

    let mut msgs = Vec::new();

    if let Some(client) = state.client.take() {
        client.endpoint.close();
        msgs.push(format!("disconnected from {}", client.remote_addr));
    }

    if let Some(server) = state.server.take() {
        server.endpoint.close();
        msgs.push(format!("server on port {} stopped", server.port));
    }

    if let Some(h) = state.sender_task.take() {
        h.abort();
        msgs.push("sender stopped".into());
    }
    if let Some(h) = state.receiver_task.take() {
        h.abort();
        msgs.push("receiver stopped".into());
    }
    if let Some(h) = state.input_task.take() {
        h.abort();
        msgs.push("input handler stopped".into());
    }
    if let Some(h) = state.audio_sender_task.take() {
        h.abort();
        msgs.push("audio sender stopped".into());
    }
    if let Some(h) = state.audio_receiver_task.take() {
        h.abort();
        msgs.push("audio receiver stopped".into());
    }
    if let Some(h) = state.clipboard_task.take() {
        h.abort();
        msgs.push("clipboard sync stopped".into());
    }
    if let Some(h) = state.reconnect_task.take() {
        h.abort();
        msgs.push("reconnect supervisor stopped".into());
    }
    if let Some(h) = state.quality_task.take() {
        h.abort();
    }

    *frame_rx_lock().lock().await = None;
    *input_transport_lock().lock().await = None;

    state.video_active = false;
    state.last_pipeline_error = None;
    state.frames_received = 0;
    state.file_progress = FileTransferProgress::default();

    if msgs.is_empty() {
        "already disconnected".to_string()
    } else {
        msgs.join("; ")
    }
}

/// Get the local peer ID (stable across calls).
pub fn get_peer_id() -> String {
    ensure_peer_id().to_string()
}

/// Simple ping for testing the bridge.
pub fn ping(msg: String) -> String {
    format!("pong: {}", msg)
}

/// Run a diagnostic self-test: check server status, discovery socket, and local IPs.
/// Returns a multi-line diagnostic report.
pub async fn run_diagnostics() -> String {
    let mut lines = Vec::new();

    // 1. Peer ID
    lines.push(format!("Peer ID: {}", ensure_peer_id()));

    // 2. Server status
    let state = app_state().read().await;
    if let Some(ref srv) = state.server {
        lines.push(format!("Server: listening on port {}", srv.port));
    } else {
        lines.push("Server: NOT running".into());
    }
    let peer_count = state.discovered_peers.len();
    drop(state);
    lines.push(format!("Discovered peers: {}", peer_count));

    // 3. Test UDP discovery socket bind
    match alldesk_net::LanDiscovery::test_socket() {
        Ok(_) => lines.push("Discovery socket: OK (bind to 0.0.0.0:21117)".into()),
        Err(e) => lines.push(format!("Discovery socket: FAILED ({})", e)),
    }

    // 4. Test broadcast send
    match alldesk_net::LanDiscovery::test_broadcast() {
        Ok(_) => lines.push("Broadcast send: OK".into()),
        Err(e) => lines.push(format!("Broadcast send: FAILED ({})", e)),
    }

    // 5. Local IP addresses
    match local_ip_addresses() {
        Ok(ips) => lines.push(format!("Local IPs: {}", ips.join(", "))),
        Err(e) => lines.push(format!("Local IPs: error ({})", e)),
    }

    lines.join("\n")
}

fn local_ip_addresses() -> Result<Vec<String>, String> {
    let mut ips = Vec::new();
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind: {}", e))?;
    sock.connect("8.8.8.8:80")
        .map_err(|e| format!("connect: {}", e))?;
    let local = sock
        .local_addr()
        .map_err(|e| format!("local_addr: {}", e))?;
    ips.push(local.ip().to_string());
    Ok(ips)
}

// ---------- Streaming APIs ----------

/// Start sending screen capture to the connected peer (host side).
/// NOTE: The server now auto-accepts connections in `start_server_internal()`.
/// This function is kept for backward compatibility but is no longer required.
pub async fn start_screen_stream(_bitrate_kbps: u32, _fps: u32) -> Result<String, String> {
    Ok("auto-accept already active — viewer just needs to connect".into())
}

/// Record a pipeline error so `get_connection_quality()` can surface it.
async fn record_pipeline_error(msg: String) {
    app_state().write().await.last_pipeline_error = Some(msg);
}

/// Receive input events from the viewer and inject them into the local OS.
async fn run_input_handler(
    conn: quinn::Connection,
    streams: alldesk_net::transport::SharedStreams,
) {
    let mut transport =
        alldesk_net::transport::QuicTransport::with_shared_streams(conn, true, streams);

    // Wait for the Input stream opened by the viewer (may happen only when
    // the user first moves the mouse, so no timeout). Streams for sibling
    // pipelines are registered in the shared table and left alone.
    if let Err(e) = transport.accept_channel(Channel::Input).await {
        tracing::warn!("accept input stream: {}", e);
        return;
    }
    tracing::info!("input stream accepted");

    // Platform-specific input controller
    #[cfg(target_os = "windows")]
    let controller = alldesk_platform::input::WindowsInputController::new();
    #[cfg(target_os = "macos")]
    let controller = alldesk_platform::input::MacInputController::new();
    #[cfg(target_os = "android")]
    let controller = alldesk_platform::input::AndroidInputController::new();

    loop {
        match transport.recv(Channel::Input).await {
            Ok(msg) => {
                if msg.is_empty() {
                    continue;
                }
                let msg_type = msg[0];
                match msg_type {
                    0x01 => handle_mouse_event(&controller, &msg),
                    0x02 => handle_scroll_event(&controller, &msg),
                    0x03 => handle_key_event(&controller, &msg),
                    _ => tracing::warn!("unknown input msg type: 0x{:02x}", msg_type),
                }
            }
            Err(e) => {
                // Stream/transport errors are fatal (connection closed);
                // retrying forever would just spin and spam the log.
                tracing::warn!("recv input: {}", e);
                return;
            }
        }
    }
}

fn handle_mouse_event(controller: &dyn InputController, msg: &[u8]) {
    // Format: [0x01] [x: f64 LE] [y: f64 LE] [action: UTF-8]
    if msg.len() < 17 {
        return;
    }
    let x = f64::from_le_bytes(msg[1..9].try_into().unwrap());
    let y = f64::from_le_bytes(msg[9..17].try_into().unwrap());
    let action = String::from_utf8_lossy(&msg[17..]);

    if let Err(e) = controller.mouse_move(x as i32, y as i32, false) {
        tracing::warn!("mouse_move: {}", e);
        return;
    }

    match action.as_ref() {
        "down" => {
            if let Err(e) = controller.mouse_click(MouseButton::Left, ButtonState::Pressed) {
                tracing::warn!("mouse_click down: {}", e);
            }
        }
        "up" => {
            if let Err(e) = controller.mouse_click(MouseButton::Left, ButtonState::Released) {
                tracing::warn!("mouse_click up: {}", e);
            }
        }
        "move" => {}
        "right_down" => {
            if let Err(e) = controller.mouse_click(MouseButton::Right, ButtonState::Pressed) {
                tracing::warn!("mouse_click right_down: {}", e);
            }
        }
        "right_up" => {
            if let Err(e) = controller.mouse_click(MouseButton::Right, ButtonState::Released) {
                tracing::warn!("mouse_click right_up: {}", e);
            }
        }
        "middle_down" => {
            if let Err(e) = controller.mouse_click(MouseButton::Middle, ButtonState::Pressed) {
                tracing::warn!("mouse_click middle_down: {}", e);
            }
        }
        "middle_up" => {
            if let Err(e) = controller.mouse_click(MouseButton::Middle, ButtonState::Released) {
                tracing::warn!("mouse_click middle_up: {}", e);
            }
        }
        _ => tracing::warn!("unknown mouse action: {}", action),
    }
}

fn handle_scroll_event(controller: &dyn InputController, msg: &[u8]) {
    // Format: [0x02] [dy: f64 LE]
    if msg.len() < 9 {
        return;
    }
    let dy = f64::from_le_bytes(msg[1..9].try_into().unwrap());
    let delta = (dy * 120.0) as i32;
    if let Err(e) = controller.mouse_scroll(0, delta) {
        tracing::warn!("mouse_scroll: {}", e);
    }
}

fn handle_key_event(controller: &dyn InputController, msg: &[u8]) {
    // Format: [0x03] [state: u8 0=press 1=release] [key_type: u8] [payload...]
    // key_type 0x01 = char (4 bytes LE), 0x02 = special key (1 byte enum)
    if msg.len() < 4 {
        return;
    }
    let state = match msg[1] {
        0 => KeyState::Pressed,
        _ => KeyState::Released,
    };
    let key_type = msg[2];

    match key_type {
        0x01 => {
            // Unicode char
            if msg.len() < 7 {
                return;
            }
            let code_point = u32::from_le_bytes(msg[3..7].try_into().unwrap());
            if let Some(ch) = char::from_u32(code_point) {
                if state == KeyState::Pressed {
                    if let Err(e) = controller.unicode_char(ch) {
                        tracing::warn!("unicode_char: {}", e);
                    }
                }
            }
        }
        0x02 => {
            // Special key
            if msg.len() < 4 {
                return;
            }
            let key = decode_special_key(msg[3]);
            if let Err(e) = controller.key_event(key, state) {
                tracing::warn!("key_event: {}", e);
            }
        }
        _ => tracing::warn!("unknown key_type: 0x{:02x}", key_type),
    }
}

fn decode_special_key(code: u8) -> KeyCode {
    match code {
        0x01 => KeyCode::Enter,
        0x02 => KeyCode::Escape,
        0x03 => KeyCode::Tab,
        0x04 => KeyCode::Backspace,
        0x05 => KeyCode::Delete,
        0x06 => KeyCode::ArrowUp,
        0x07 => KeyCode::ArrowDown,
        0x08 => KeyCode::ArrowLeft,
        0x09 => KeyCode::ArrowRight,
        n if n >= 0x10 => KeyCode::Function(n - 0x10),
        _ => KeyCode::Unknown(code as u32),
    }
}

/// Poll for the next available video frame (viewer side).
/// Returns [width_u32_le, height_u32_le, ...bgra_data] or None.
pub async fn poll_video_frame() -> Option<Vec<u8>> {
    let mut rx_guard = frame_rx_lock().lock().await;
    let rx = rx_guard.as_mut()?;
    match rx.try_recv() {
        Ok(frame) => {
            // Track that video is flowing
            if let Ok(mut state) = app_state().try_write() {
                state.video_active = true;
                state.frames_received += 1;
                if let Ok(mut q) = state.quality.lock() {
                    q.record_frame_received();
                }
            }
            let mut out = Vec::with_capacity(8 + frame.bgra_data.len());
            out.extend_from_slice(&frame.width.to_le_bytes());
            out.extend_from_slice(&frame.height.to_le_bytes());
            out.extend_from_slice(&frame.bgra_data);
            Some(out)
        }
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
            // The UI consumed too slowly; skipped frames count as dropped.
            if let Ok(state) = app_state().try_write() {
                if let Ok(mut q) = state.quality.lock() {
                    for _ in 0..skipped {
                        q.record_frame_dropped();
                    }
                }
            }
            None
        }
        Err(_) => None,
    }
}

/// Send a mouse event to the remote peer (viewer side).
pub async fn send_mouse_event(x: f64, y: f64, action: String) -> Result<(), String> {
    let action_bytes = action.as_bytes();
    let mut msg = Vec::with_capacity(1 + 8 + 8 + action_bytes.len());
    msg.push(0x01);
    msg.extend_from_slice(&x.to_le_bytes());
    msg.extend_from_slice(&y.to_le_bytes());
    msg.extend_from_slice(action_bytes);

    // Take the transport out, send without holding the lock, then put it back.
    let mut transport = input_transport_lock()
        .lock()
        .await
        .take()
        .ok_or_else(|| "not connected".to_string())?;
    let result = transport
        .send(Channel::Input, &msg)
        .await
        .map_err(|e| format!("send mouse: {}", e));
    input_transport_lock().lock().await.replace(transport);
    result
}

/// Send a scroll event to the remote peer.
pub async fn send_scroll(dy: f64) -> Result<(), String> {
    let mut msg = Vec::with_capacity(1 + 8);
    msg.push(0x02);
    msg.extend_from_slice(&dy.to_le_bytes());

    let mut transport = input_transport_lock()
        .lock()
        .await
        .take()
        .ok_or_else(|| "not connected".to_string())?;
    let result = transport
        .send(Channel::Input, &msg)
        .await
        .map_err(|e| format!("send scroll: {}", e));
    input_transport_lock().lock().await.replace(transport);
    result
}

/// Send a key event to the remote peer (viewer side).
/// For char keys: key_type="char", key is the unicode codepoint.
/// For special keys: key_type="special", key is the special key code.
/// pressed: true = key down, false = key up.
pub async fn send_key_event(key_type: String, key: u32, pressed: bool) -> Result<(), String> {
    let mut msg = Vec::with_capacity(16);
    msg.push(0x03); // input msg type: key
    msg.push(if pressed { 0 } else { 1 }); // state

    match key_type.as_ref() {
        "char" => {
            msg.push(0x01); // key_type: char
            msg.extend_from_slice(&key.to_le_bytes());
        }
        "special" => {
            msg.push(0x02); // key_type: special
            msg.push(key as u8);
        }
        _ => return Err(format!("unknown key_type: {}", key_type)),
    }

    let mut transport = input_transport_lock()
        .lock()
        .await
        .take()
        .ok_or_else(|| "not connected".to_string())?;
    let result = transport
        .send(Channel::Input, &msg)
        .await
        .map_err(|e| format!("send key: {}", e));
    input_transport_lock().lock().await.replace(transport);
    result
}

/// Get connection quality metrics and stream status.
/// Returns a JSON string with connection state, video status and real
/// transport metrics (RTT / packet loss / bandwidth) from the session sampler.
pub async fn get_connection_quality() -> String {
    let state = app_state().read().await;
    let has_client = state.client.is_some();
    let has_server = state.server.is_some();
    let video_active = state.video_active;
    let frames_received = state.frames_received;
    let last_error = state.last_pipeline_error.clone();
    let quality: ConnectionQuality = state
        .quality
        .lock()
        .map(|mut q| q.compute_quality())
        .unwrap_or(ConnectionQuality {
            rtt_ms: 0.0,
            packet_loss: 0.0,
            bandwidth_kbps: 0,
            quality: crate::connection_quality::QualityLevel::Bad,
            last_updated_ms: 0,
            frames_received: 0,
            frames_dropped: 0,
        });
    drop(state);

    let rx_count = {
        let rx_guard = frame_rx_lock().lock().await;
        if rx_guard.is_some() {
            "active"
        } else {
            "none"
        }
    };

    let input_ok = {
        let inp_guard = input_transport_lock().lock().await;
        if inp_guard.is_some() {
            "active"
        } else {
            "none"
        }
    };

    serde_json::json!({
        "client": has_client,
        "server": has_server,
        "video_rx": rx_count,
        "input_tx": input_ok,
        "video_active": video_active,
        "frames_received": frames_received,
        "rtt_ms": quality.rtt_ms,
        "packet_loss": quality.packet_loss,
        "bandwidth_kbps": quality.bandwidth_kbps,
        "quality": format!("{:?}", quality.quality),
        "frames_dropped": quality.frames_dropped,
        "last_error": last_error,
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// File transfer (viewer → host push over the File channel)
// ---------------------------------------------------------------------------

/// Wire format on the File channel (each message is length-prefixed by the
/// transport): header announces the file, chunks carry the data.
const FILE_MSG_HEADER: u8 = 0x01;
const FILE_MSG_CHUNK: u8 = 0x02;
const FILE_MSG_END: u8 = 0x03;

/// Where the host stores files received from viewers.
fn received_files_dir() -> std::path::PathBuf {
    let base = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("Downloads").join("AllDesk")
}

/// Strip any directory components from a file name coming from the network.
fn sanitize_remote_filename(name: &str) -> String {
    let stripped = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = stripped
        .chars()
        .filter(|c| {
            !c.is_control()
                && *c != ':'
                && *c != '*'
                && *c != '?'
                && *c != '"'
                && *c != '<'
                && *c != '>'
                && *c != '|'
        })
        .collect();
    if cleaned.is_empty() {
        "received_file".into()
    } else {
        cleaned
    }
}

/// Host side: receive files pushed by the viewer, writing them to
/// `<user>/Downloads/AllDesk/`. One task per connection; exits with the
/// connection.
async fn run_file_receiver(mut transport: alldesk_net::transport::QuicTransport) {
    // The viewer may never send a file — wait without a timeout.
    if let Err(e) = transport.accept_channel(Channel::File).await {
        tracing::debug!("file receiver: {}", e);
        return;
    }
    tracing::info!("file channel ready");

    let mut current: Option<(String, u64, tokio::fs::File)> = None;
    loop {
        let msg = match transport.recv(Channel::File).await {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!("recv file: {}", e);
                return;
            }
        };
        if msg.is_empty() {
            continue;
        }
        match msg[0] {
            FILE_MSG_HEADER if msg.len() > 3 => {
                let name_len = u16::from_le_bytes([msg[1], msg[2]]) as usize;
                if msg.len() < 3 + name_len + 8 {
                    tracing::warn!("malformed file header");
                    continue;
                }
                let name =
                    sanitize_remote_filename(&String::from_utf8_lossy(&msg[3..3 + name_len]));
                let total = u64::from_le_bytes(
                    msg[3 + name_len..3 + name_len + 8]
                        .try_into()
                        .unwrap_or([0; 8]),
                );

                let dir = received_files_dir();
                if let Err(e) = tokio::fs::create_dir_all(&dir).await {
                    tracing::error!("create {}: {}", dir.display(), e);
                    continue;
                }
                let dest = dir.join(&name);
                match tokio::fs::File::create(&dest).await {
                    Ok(f) => {
                        tracing::info!(
                            "receiving '{}' ({} bytes) -> {}",
                            name,
                            total,
                            dest.display()
                        );
                        if let Ok(mut state) = app_state().try_write() {
                            state.file_progress = FileTransferProgress {
                                active: true,
                                direction: "receiving".into(),
                                filename: name.clone(),
                                transferred: 0,
                                total,
                                error: None,
                            };
                        }
                        current = Some((name, total, f));
                    }
                    Err(e) => tracing::error!("create {}: {}", dest.display(), e),
                }
            }
            FILE_MSG_CHUNK => {
                if let Some((_, total, file)) = current.as_mut() {
                    let data = &msg[1..];
                    if let Err(e) = file.write_all(data).await {
                        // tokio::io::AsyncWriteExt
                        tracing::error!("write chunk: {}", e);
                        current = None;
                        continue;
                    }
                    if let Ok(mut state) = app_state().try_write() {
                        let p = &mut state.file_progress;
                        p.transferred = (p.transferred + data.len() as u64).min(*total);
                    }
                }
            }
            FILE_MSG_END => {
                if let Some((name, total, mut file)) = current.take() {
                    let _ = file.flush().await;
                    tracing::info!("received '{}' ({} bytes)", name, total);
                    if let Ok(mut state) = app_state().try_write() {
                        state.file_progress.transferred = total;
                        state.file_progress.active = false;
                    }
                }
            }
            _ => tracing::warn!("unknown file message type: 0x{:02x}", msg[0]),
        }
    }
}

/// Send a local file to the connected peer (viewer → host).
/// Returns immediately; poll progress with `get_file_transfer_status`.
pub async fn send_file_to_peer(path: String) -> Result<String, String> {
    let file_transport = {
        let guard = input_transport_lock().lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "not connected".to_string())?
    };

    let filename = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());

    let meta = tokio::fs::metadata(&path)
        .await
        .map_err(|e| format!("cannot read '{}': {}", path, e))?;
    let total = meta.len();

    if let Err(e) = alldesk_files::validate::validate_file_size(total) {
        return Err(format!("{}", e));
    }

    {
        let mut state = app_state().write().await;
        state.file_progress = FileTransferProgress {
            active: true,
            direction: "sending".into(),
            filename: filename.clone(),
            transferred: 0,
            total,
            error: None,
        };
    }

    let summary = format!("sending '{}' ({} bytes)", filename, total);

    tokio::spawn(async move {
        let result = send_file_session(file_transport, &path, &filename, total).await;
        let mut state = app_state().write().await;
        state.file_progress.active = false;
        if let Err(e) = result {
            tracing::error!("file transfer failed: {}", e);
            state.file_progress.error = Some(format!("{}", e));
        } else {
            state.file_progress.transferred = total;
        }
    });

    Ok(summary)
}

async fn send_file_session(
    mut transport: alldesk_net::transport::QuicTransport,
    path: &str,
    filename: &str,
    total: u64,
) -> alldesk_core::Result<()> {
    // Header: [type][name_len u16][name][total u64]
    let name_bytes = filename.as_bytes();
    let mut header = Vec::with_capacity(11 + name_bytes.len());
    header.push(FILE_MSG_HEADER);
    header.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    header.extend_from_slice(name_bytes);
    header.extend_from_slice(&total.to_le_bytes());
    transport.send(Channel::File, &header).await?;

    let transfer = FileTransfer::new();
    transfer
        .send_file(path, |chunk| {
            let mut msg = Vec::with_capacity(9 + chunk.data.len());
            msg.push(FILE_MSG_CHUNK);
            msg.extend_from_slice(&chunk.index.to_le_bytes());
            msg.extend_from_slice(&chunk.data);
            let mut t = transport.clone();
            async move { t.send(Channel::File, &msg).await }
        })
        .await?;

    transport.send(Channel::File, &[FILE_MSG_END]).await?;
    tracing::info!("sent '{}' ({} bytes)", filename, total);
    Ok(())
}

/// Current file transfer progress as JSON
/// ({active, direction, filename, transferred, total, error}).
pub async fn get_file_transfer_status() -> String {
    let state = app_state().read().await;
    serde_json::to_string(&state.file_progress).unwrap_or_else(|_| "{}".into())
}

// ---------------------------------------------------------------------------
// Session recording (viewer side; VP9 frames are stored as-is)
// ---------------------------------------------------------------------------

/// The recorder attached to the live receiver pipeline, if recording.
pub(crate) fn current_recorder() -> Option<Arc<std::sync::Mutex<Recorder>>> {
    recorder_lock()
        .try_lock()
        .ok()
        .and_then(|guard| guard.clone())
}

/// Start recording the remote session (VP9-encoded frames) to `path`.
pub async fn start_session_recording(path: String) -> Result<String, String> {
    let recorder = Recorder::new(&path).map_err(|e| format!("cannot create '{}': {}", path, e))?;
    *recorder_lock().lock().await = Some(Arc::new(std::sync::Mutex::new(recorder)));
    tracing::info!("session recording started: {}", path);
    Ok(format!("recording to {}", path))
}

/// Stop recording and finalize the file. Returns the output path.
pub async fn stop_session_recording() -> Result<String, String> {
    let rec = recorder_lock()
        .lock()
        .await
        .take()
        .ok_or_else(|| "not recording".to_string())?;
    let recorder = Arc::try_unwrap(rec)
        .map_err(|_| "recorder still in use".to_string())?
        .into_inner()
        .map_err(|_| "recorder lock poisoned".to_string())?;
    let frames = recorder.frame_count();
    recorder
        .finish()
        .map_err(|e| format!("finalize recording: {}", e))?;
    tracing::info!("session recording stopped ({} frames)", frames);
    Ok(format!("saved {} frames", frames))
}

/// Push a video frame from the Android screen capture service into Rust.
#[cfg(target_os = "android")]
#[flutter_rust_bridge::frb(sync)]
pub fn push_android_frame(bgra_data: Vec<u8>, width: u32, height: u32) -> bool {
    alldesk_capture::android::push_android_frame(bgra_data, width, height)
}

/// Stub for non-Android platforms.
#[cfg(not(target_os = "android"))]
#[flutter_rust_bridge::frb(sync)]
pub fn push_android_frame(_bgra_data: Vec<u8>, _width: u32, _height: u32) -> bool {
    false
}

/// Get Android frame capture statistics: (frames_received, frames_dropped).
#[flutter_rust_bridge::frb(sync)]
pub fn get_android_frame_stats() -> (u64, u64) {
    #[cfg(target_os = "android")]
    {
        alldesk_capture::android::get_android_frame_stats()
    }
    #[cfg(not(target_os = "android"))]
    {
        (0, 0)
    }
}

/// Stop all streaming (both sender and receiver).
pub async fn stop_stream() -> String {
    let mut state = app_state().write().await;
    let mut msgs: Vec<String> = Vec::new();

    if let Some(h) = state.sender_task.take() {
        h.abort();
        msgs.push("sender stopped".into());
    }
    if let Some(h) = state.receiver_task.take() {
        h.abort();
        msgs.push("receiver stopped".into());
    }
    if let Some(h) = state.input_task.take() {
        h.abort();
        msgs.push("input handler stopped".into());
    }
    if let Some(h) = state.audio_sender_task.take() {
        h.abort();
        msgs.push("audio sender stopped".into());
    }
    if let Some(h) = state.audio_receiver_task.take() {
        h.abort();
        msgs.push("audio receiver stopped".into());
    }
    if let Some(h) = state.clipboard_task.take() {
        h.abort();
        msgs.push("clipboard sync stopped".into());
    }
    if let Some(h) = state.reconnect_task.take() {
        h.abort();
        msgs.push("reconnect supervisor stopped".into());
    }
    if let Some(h) = state.quality_task.take() {
        h.abort();
    }
    *frame_rx_lock().lock().await = None;
    *input_transport_lock().lock().await = None;

    state.video_active = false;
    state.last_pipeline_error = None;
    state.frames_received = 0;
    state.file_progress = FileTransferProgress::default();

    if msgs.is_empty() {
        "no active stream".into()
    } else {
        msgs.join("; ")
    }
}
