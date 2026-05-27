use std::net::SocketAddr;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use alldesk_core::config::AppConfig;
use alldesk_net::{LanDiscovery, QuicEndpoint, Transport};
use alldesk_net::channel::Channel;
use crate::pipeline::{SenderPipeline, ReceiverPipeline, VideoFrame};

use alldesk_input::{InputController, MouseButton, ButtonState, KeyCode, KeyState};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(500);

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
    PEER_ID.get_or_init(|| {
        AppConfig::default().peer_id
    })
}

// ---------- Shared runtime state ----------

struct ServerState {
    endpoint: QuicEndpoint,
    port: u16,
}

struct ClientState {
    endpoint: QuicEndpoint,
    #[allow(dead_code)]
    conn: quinn::Connection,
    remote_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedPeer {
    peer_id: String,
    peer_name: String,
    address: String,
}

static APP_STATE: OnceLock<RwLock<AppState>> = OnceLock::new();

// Separate fine-grained locks to avoid contention between frame polling and input sending.
static FRAME_RX: OnceLock<Mutex<Option<tokio::sync::broadcast::Receiver<VideoFrame>>>> = OnceLock::new();
static INPUT_TRANSPORT: OnceLock<Mutex<Option<alldesk_net::transport::QuicTransport>>> = OnceLock::new();

fn frame_rx_lock() -> &'static Mutex<Option<tokio::sync::broadcast::Receiver<VideoFrame>>> {
    FRAME_RX.get_or_init(|| Mutex::new(None))
}

fn input_transport_lock() -> &'static Mutex<Option<alldesk_net::transport::QuicTransport>> {
    INPUT_TRANSPORT.get_or_init(|| Mutex::new(None))
}

struct AppState {
    server: Option<ServerState>,
    client: Option<ClientState>,
    sender_task: Option<tokio::task::JoinHandle<()>>,
    receiver_task: Option<tokio::task::JoinHandle<()>>,
    input_task: Option<tokio::task::JoinHandle<()>>,
    discovered_peers: Vec<CachedPeer>,
}

fn app_state() -> &'static RwLock<AppState> {
    APP_STATE.get_or_init(|| {
        RwLock::new(AppState {
            server: None,
            client: None,
            sender_task: None,
            receiver_task: None,
            input_task: None,
            discovered_peers: Vec::new(),
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
            if let Err(e) = LanDiscovery::discover_loop(
                peer_id,
                peer_name,
                21116,
                tx,
            ).await {
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
            if let Some(existing) = state.discovered_peers.iter_mut().find(|p| p.peer_id == cached.peer_id) {
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
        match tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            discovery.discover(),
        ).await {
            Ok(Ok(peers)) => {
                let mut state = app_state().write().await;
                for peer in peers {
                    let cached = CachedPeer {
                        peer_id: peer.peer_id.clone(),
                        peer_name: peer.peer_name.clone(),
                        address: peer.addr.to_string(),
                    };
                    if let Some(existing) = state.discovered_peers.iter_mut()
                        .find(|p| p.peer_id == cached.peer_id) {
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
    state.discovered_peers.iter().map(|p| PeerInfo {
        peer_id: p.peer_id.clone(),
        peer_name: p.peer_name.clone(),
        address: p.address.clone(),
    }).collect()
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

    let local = endpoint.local_addr()
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

                    let input_conn = conn.clone();
                    let video_transport = alldesk_net::transport::QuicTransport::new(conn, true);

                    // Start input handler concurrently (host-side: receive viewer input → inject to OS)
                    let input_handle = tokio::spawn(run_input_handler(input_conn));

                    // Start sender pipeline concurrently (capture → send raw BGRA to viewer)
                    let sender_handle = tokio::spawn(async move {
                        match SenderPipeline::new(video_transport, 4000, 30).await {
                            Ok(mut pipeline) => {
                                if let Err(e) = pipeline.run().await {
                                    tracing::error!("sender pipeline: {}", e);
                                }
                            }
                            Err(e) => {
                                tracing::error!("create sender pipeline: {}", e);
                            }
                        }
                    });

                    // Wait for either to finish (connection closed)
                    let (sender_result, input_result) = tokio::join!(sender_handle, input_handle);
                    let _ = (sender_result, input_result);
                    tracing::info!("viewer session ended");
                }
                Err(e) => {
                    tracing::error!("accept connection failed: {}", e);
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
    let remote: SocketAddr = addr.parse()
        .map_err(|e| format!("invalid address '{}': {}", addr, e))?;

    let endpoint = QuicEndpoint::new_client()
        .map_err(|e| format!("failed to create client endpoint: {}", e))?;

    // Retry loop with timeout on each attempt
    let mut last_err = String::new();
    for attempt in 1..=MAX_RETRIES {
        match tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(remote)).await {
            Ok(Ok(conn)) => {
                let remote_id = conn.remote_address().to_string();

                let transport = alldesk_net::transport::QuicTransport::new(conn.clone(), true);
                let (mut rx_pipeline, frame_rx) = ReceiverPipeline::new(transport, 1920, 1080);

                let receiver_task = tokio::spawn(async move {
                    if let Err(e) = rx_pipeline.run().await {
                        tracing::error!("receiver pipeline: {}", e);
                    }
                });

                let input_transport = alldesk_net::transport::QuicTransport::new(conn.clone(), true);

                let mut state = app_state().write().await;
                if let Some(old) = state.client.take() {
                    old.endpoint.close();
                }
                if let Some(h) = state.receiver_task.take() {
                    h.abort();
                }

                state.client = Some(ClientState {
                    endpoint,
                    conn,
                    remote_addr: addr.clone(),
                });
                state.receiver_task = Some(receiver_task);

                // Store frame_rx and input_transport in their own fine-grained locks
                *frame_rx_lock().lock().await = Some(frame_rx);
                *input_transport_lock().lock().await = Some(input_transport);

                tracing::info!("connected to {} ({})", addr, remote_id);
                return Ok(format!("connected to {} ({})", addr, remote_id));
            }
            Ok(Err(e)) => {
                last_err = format!("connection to {} failed: {}", addr, e);
                tracing::warn!("connect attempt {}/{}: {}", attempt, MAX_RETRIES, last_err);
            }
            Err(_) => {
                last_err = format!("connection to {} timed out after {:?}", addr, CONNECT_TIMEOUT);
                tracing::warn!("connect attempt {}/{}: timed out", attempt, MAX_RETRIES);
            }
        }

        if attempt < MAX_RETRIES {
            tokio::time::sleep(RETRY_DELAY * attempt).await;
        }
    }

    Err(last_err)
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

    *frame_rx_lock().lock().await = None;
    *input_transport_lock().lock().await = None;

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
    let sock = std::net::UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("bind: {}", e))?;
    sock.connect("8.8.8.8:80")
        .map_err(|e| format!("connect: {}", e))?;
    let local = sock.local_addr()
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

/// Receive input events from the viewer and inject them into the local OS.
async fn run_input_handler(conn: quinn::Connection) {
    let mut transport = alldesk_net::transport::QuicTransport::new(conn, true);

    // Accept the Input stream opened by the viewer
    match transport.accept_stream().await {
        Ok(ch) => {
            if ch != Channel::Input {
                tracing::warn!("expected Input channel, got {:?}", ch);
                return;
            }
        }
        Err(e) => {
            tracing::error!("accept input stream: {}", e);
            return;
        }
    }

    // Platform-specific input controller
    #[cfg(target_os = "windows")]
    let controller = alldesk_input::WindowsInputController::new();
    #[cfg(target_os = "macos")]
    let controller = alldesk_input::MacInputController::new();
    #[cfg(target_os = "android")]
    let controller = alldesk_input::AndroidInputController::new();

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
                tracing::warn!("recv input: {}", e);
                tokio::time::sleep(std::time::Duration::from_millis(16)).await;
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
            if msg.len() < 7 { return; }
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
            if msg.len() < 4 { return; }
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
            let mut out = Vec::with_capacity(8 + frame.bgra_data.len());
            out.extend_from_slice(&frame.width.to_le_bytes());
            out.extend_from_slice(&frame.height.to_le_bytes());
            out.extend_from_slice(&frame.bgra_data);
            Some(out)
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
    let mut transport = input_transport_lock().lock().await.take()
        .ok_or_else(|| "not connected".to_string())?;
    let result = transport.send(Channel::Input, &msg).await
        .map_err(|e| format!("send mouse: {}", e));
    input_transport_lock().lock().await.replace(transport);
    result
}

/// Send a scroll event to the remote peer.
pub async fn send_scroll(dy: f64) -> Result<(), String> {
    let mut msg = Vec::with_capacity(1 + 8);
    msg.push(0x02);
    msg.extend_from_slice(&dy.to_le_bytes());

    let mut transport = input_transport_lock().lock().await.take()
        .ok_or_else(|| "not connected".to_string())?;
    let result = transport.send(Channel::Input, &msg).await
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

    let mut transport = input_transport_lock().lock().await.take()
        .ok_or_else(|| "not connected".to_string())?;
    let result = transport.send(Channel::Input, &msg).await
        .map_err(|e| format!("send key: {}", e));
    input_transport_lock().lock().await.replace(transport);
    result
}

/// Get connection quality metrics.
/// Returns a JSON string with rtt_ms, packet_loss, bandwidth_kbps, quality level.
pub async fn get_connection_quality() -> String {
    let state = app_state().read().await;
    let has_client = state.client.is_some();
    let has_server = state.server.is_some();
    drop(state);

    let rx_count = {
        let rx_guard = frame_rx_lock().lock().await;
        if rx_guard.is_some() { "active" } else { "none" }
    };

    let input_ok = {
        let inp_guard = input_transport_lock().lock().await;
        if inp_guard.is_some() { "active" } else { "none" }
    };

    serde_json::json!({
        "client": has_client,
        "server": has_server,
        "video_rx": rx_count,
        "input_tx": input_ok,
    }).to_string()
}

/// Push a video frame from the Android screen capture service into Rust.
/// Returns true if the frame was accepted, false if dropped.
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
    *frame_rx_lock().lock().await = None;
    *input_transport_lock().lock().await = None;

    if msgs.is_empty() {
        "no active stream".into()
    } else {
        msgs.join("; ")
    }
}
