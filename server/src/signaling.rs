use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

use crate::registry::{PeerListEntry, PeerRecord, PeerRegistry, SignalingMessage};
use crate::relay::RelayServer;

/// TLS configuration for the signaling server. If provided, connections use WSS.
#[derive(Clone)]
pub struct TlsConfig {
    /// Loaded TLS acceptor from cert/key files.
    acceptor: Arc<tokio_rustls::TlsAcceptor>,
}

impl TlsConfig {
    /// Load TLS configuration from PEM-encoded certificate and key files.
    pub fn from_files(cert_path: &str, key_path: &str) -> anyhow::Result<Self> {
        let cert_file = std::fs::File::open(cert_path)?;
        let key_file = std::fs::File::open(key_path)?;

        let certs: Vec<_> = rustls_pemfile::certs(&mut std::io::BufReader::new(cert_file))
            .collect::<Result<Vec<_>, _>>()?;
        let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(key_file))?
            .ok_or_else(|| anyhow::anyhow!("no private key found in {}", key_path))?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        Ok(Self { acceptor: Arc::new(acceptor) })
    }
}

/// Maximum WebSocket message size (64 KB).
const MAX_MESSAGE_SIZE: usize = 64 * 1024;

/// Maximum messages per second per connection before rate limiting kicks in.
const MAX_MESSAGES_PER_SECOND: u64 = 30;

/// A broadcaster that can send messages to connected WebSocket peers.
#[derive(Debug, Clone)]
pub struct SignalingBroadcaster {
    senders: Arc<Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<Message>>>>,
}

/// Simple token-based authentication for the signaling server.
/// If a shared secret token is configured, all peers must provide it during registration.
#[derive(Debug, Clone)]
pub struct ServerAuth {
    /// Shared secret token. If None, authentication is disabled (LAN-trust mode).
    token: Option<String>,
}

impl ServerAuth {
    /// Create auth with no token requirement (LAN-trust mode).
    #[allow(dead_code)]
    pub fn no_auth() -> Self {
        Self { token: None }
    }

    /// Create auth requiring the given token.
    #[allow(dead_code)]
    pub fn with_token(token: String) -> Self {
        Self { token: Some(token) }
    }

    /// Load token from environment variable or use None.
    pub fn from_env() -> Self {
        match std::env::var("ALLDESK_AUTH_TOKEN") {
            Ok(t) if !t.is_empty() => {
                tracing::info!("Server authentication enabled (token from env)");
                Self { token: Some(t) }
            }
            _ => {
                tracing::info!("Server authentication disabled (no token configured)");
                Self { token: None }
            }
        }
    }

    /// Check if authentication is enabled.
    #[allow(dead_code)]
    pub fn is_enabled(&self) -> bool {
        self.token.is_some()
    }

    /// Verify a provided token. Returns true if auth is disabled or token matches.
    pub fn verify(&self, provided: Option<&str>) -> bool {
        match &self.token {
            None => true,
            Some(required) => provided == Some(required.as_str()),
        }
    }
}

impl SignalingBroadcaster {
    pub fn new() -> Self {
        Self {
            senders: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a sender for a peer.
    pub async fn register_sender(
        &self,
        peer_id: String,
        tx: tokio::sync::mpsc::UnboundedSender<Message>,
    ) {
        self.senders.lock().await.insert(peer_id, tx);
    }

    /// Remove a sender for a peer.
    pub async fn unregister_sender(&self, peer_id: &str) {
        self.senders.lock().await.remove(peer_id);
    }

    /// Send a message to a specific peer.
    pub async fn send_to(&self, peer_id: &str, msg: SignalingMessage) -> bool {
        let senders = self.senders.lock().await;
        if let Some(tx) = senders.get(peer_id) {
            if tx.send(msg.to_ws_message()).is_err() {
                warn!("Failed to send message to peer {}: channel closed", peer_id);
                return false;
            }
            true
        } else {
            debug!("No sender for peer {}", peer_id);
            false
        }
    }
}

/// Per-connection rate limiter state.
struct RateLimiter {
    /// Timestamps of messages within the current window.
    timestamps: Vec<std::time::Instant>,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            timestamps: Vec::new(),
        }
    }

    /// Check if a message is allowed. Returns true if within rate limits.
    fn check(&mut self) -> bool {
        let now = std::time::Instant::now();
        let window = Duration::from_secs(1);

        // Remove timestamps outside the window
        self.timestamps.retain(|t| now.duration_since(*t) < window);

        if self.timestamps.len() >= MAX_MESSAGES_PER_SECOND as usize {
            return false;
        }

        self.timestamps.push(now);
        true
    }
}

/// Validate an incoming WebSocket message. Returns Ok for valid messages,
/// Err with a reason string for invalid ones.
fn validate_message(msg: &Message) -> Result<(), String> {
    match msg {
        Message::Text(text) => {
            if text.len() > MAX_MESSAGE_SIZE {
                return Err(format!(
                    "Message too large: {} bytes (max {})",
                    text.len(),
                    MAX_MESSAGE_SIZE
                ));
            }

            // Try to parse as JSON to verify it's valid signaling message
            let parsed: serde_json::Value = serde_json::from_str(text)
                .map_err(|e| format!("Invalid JSON: {}", e))?;

            // Verify it has a "type" field
            let msg_type = parsed.get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'type' field".to_string())?;

            // Validate known message types
            const VALID_TYPES: &[&str] = &[
                "register", "lookup", "lookup_response",
                "connect_request", "connect_accept", "connect_reject",
                "incoming_connection", "relay_request", "relay_assigned",
                "list_peers", "peer_list", "heartbeat", "heartbeat_ack",
                "error",
            ];

            if !VALID_TYPES.contains(&msg_type) {
                return Err(format!("Unknown message type: {}", msg_type));
            }

            Ok(())
        }
        Message::Close(_) => Ok(()),
        _ => Err("Unsupported message type (expected text)".to_string()),
    }
}

/// Run the WebSocket signaling server.
pub async fn run_signaling_server(
    port: u16,
    registry: PeerRegistry,
    broadcaster: SignalingBroadcaster,
    relay_server: RelayServer,
    shutdown: Arc<AtomicBool>,
    auth: ServerAuth,
    tls_config: Option<TlsConfig>,
) -> anyhow::Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let scheme = if tls_config.is_some() { "wss" } else { "ws" };
    info!("Signaling server listening on {}://{}", scheme, addr);

    // Periodic cleanup of expired peers
    let cleanup_registry = registry.clone();
    let cleanup_shutdown = shutdown.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if cleanup_shutdown.load(Ordering::Relaxed) {
                break;
            }
            let removed = cleanup_registry.cleanup_expired().await;
            if removed > 0 {
                debug!("Cleaned up {} expired peers", removed);
            }
        }
    });

    // Accept WebSocket connections
    loop {
        if shutdown.load(Ordering::Relaxed) {
            info!("Signaling server shutting down gracefully");
            break;
        }

        tokio::select! {
            accept_result = listener.accept() => {
                let (stream, addr) = accept_result?;
                info!("New TCP connection from {}", addr);

                let registry = registry.clone();
                let broadcaster = broadcaster.clone();
                let relay = relay_server.clone();
                let conn_auth = auth.clone();
                let tls = tls_config.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_connection_with_optional_tls(
                        stream, addr, registry, broadcaster, relay, conn_auth, tls,
                    ).await {
                        error!("WebSocket connection error from {}: {}", addr, e);
                    }
                });
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

    info!("Signaling server stopped accepting new connections");
    Ok(())
}

/// Handle a connection with optional TLS wrapping.
async fn handle_connection_with_optional_tls(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    registry: PeerRegistry,
    broadcaster: SignalingBroadcaster,
    relay: RelayServer,
    auth: ServerAuth,
    tls: Option<TlsConfig>,
) -> anyhow::Result<()> {
    if let Some(tls_config) = tls {
        let tls_stream = tls_config.acceptor.accept(stream).await?;
        handle_ws_connection(tls_stream, addr, registry, broadcaster, relay, auth).await
    } else {
        handle_ws_connection(stream, addr, registry, broadcaster, relay, auth).await
    }
}

/// Handle a single WebSocket connection over any stream type.
async fn handle_ws_connection<S>(
    stream: S,
    addr: SocketAddr,
    registry: PeerRegistry,
    broadcaster: SignalingBroadcaster,
    relay: RelayServer,
    auth: ServerAuth,
) -> anyhow::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    info!("WebSocket established with {}", addr);

    crate::metrics::record_ws_connection();

    let (ws_sink, ws_stream_rx) = ws_stream.split();

    // Channel for sending messages back to this client
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    // Track which peer_id this connection is registered as
    let peer_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    // Forward messages from the channel to the WebSocket sink
    let forward_peer_id = peer_id.clone();
    let forward_broadcaster = broadcaster.clone();
    let forward_registry = registry.clone();
    let send_task = tokio::spawn(async move {
        use futures::SinkExt;
        let mut sink = ws_sink;
        while let Some(msg) = rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
        // Cleanup when the send loop ends
        let pid = forward_peer_id.lock().await;
        if let Some(ref id) = *pid {
            forward_registry.unregister(id).await;
            forward_broadcaster.unregister_sender(id).await;
        }
    });

    // Read messages from the WebSocket
    let read_registry = registry.clone();
    let read_broadcaster = broadcaster.clone();
    let read_relay = relay;
    let read_peer_id = peer_id.clone();
    let read_addr = addr;

    use futures::StreamExt;
    let mut ws_rx = ws_stream_rx;

    let mut rate_limiter = RateLimiter::new();

    while let Some(msg_result) = ws_rx.next().await {
        match msg_result {
            Ok(msg) => {
                if msg.is_close() {
                    debug!("WebSocket close from {}", read_addr);
                    break;
                }

                // Rate limiting
                if !rate_limiter.check() {
                    warn!("Rate limiting connection from {}", read_addr);
                    let _ = tx.send(
                        SignalingMessage::Error {
                            message: "Rate limit exceeded".into(),
                        }
                        .to_ws_message(),
                    );
                    continue;
                }

                // Message validation
                if let Err(reason) = validate_message(&msg) {
                    warn!("Invalid message from {}: {}", read_addr, reason);
                    let _ = tx.send(
                        SignalingMessage::Error {
                            message: format!("Invalid message: {}", reason),
                        }
                        .to_ws_message(),
                    );
                    continue;
                }

                let signaling_msg = match SignalingMessage::from_ws_message(&msg) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(
                            "Invalid signaling message from {}: {}",
                            read_addr, e
                        );
                        let _ = tx.send(
                            SignalingMessage::Error {
                                message: format!("Invalid message: {}", e),
                            }
                            .to_ws_message(),
                        );
                        continue;
                    }
                };

                // Record signaling message metric
                crate::metrics::record_signaling_message(signaling_msg.type_name());

                handle_signaling_message(
                    signaling_msg,
                    &read_addr,
                    &read_registry,
                    &read_broadcaster,
                    &read_relay,
                    &read_peer_id,
                    &tx,
                    &auth,
                )
                .await;
            }
            Err(e) => {
                warn!("WebSocket error from {}: {}", read_addr, e);
                break;
            }
        }
    }

    // Cleanup
    {
        let pid = peer_id.lock().await;
        if let Some(ref id) = *pid {
            registry.unregister(id).await;
            broadcaster.unregister_sender(id).await;
            info!("Peer {} disconnected from {}", id, addr);
            crate::metrics::record_ws_disconnection();
        }
    }

    send_task.abort();
    Ok(())
}

/// Handle a single signaling message.
#[allow(clippy::too_many_arguments)]
async fn handle_signaling_message(
    msg: SignalingMessage,
    addr: &SocketAddr,
    registry: &PeerRegistry,
    broadcaster: &SignalingBroadcaster,
    relay: &RelayServer,
    peer_id: &Arc<Mutex<Option<String>>>,
    tx: &tokio::sync::mpsc::UnboundedSender<Message>,
    auth: &ServerAuth,
) {
    match msg {
        SignalingMessage::Register {
            peer_id: pid,
            peer_name,
            nat_type,
            auth_token,
        } => {
            // Verify authentication if enabled
            if !auth.verify(auth_token.as_deref()) {
                warn!("Authentication failed for peer {} from {}", pid, addr);
                let _ = tx.send(
                    SignalingMessage::Error {
                        message: "Authentication failed".into(),
                    }
                    .to_ws_message(),
                );
                return;
            }
            let record = PeerRecord {
                peer_id: pid.clone(),
                peer_name,
                address: *addr,
                nat_type,
                last_seen: std::time::Instant::now(),
            };

            let is_new = registry.register(record).await;

            // Track the peer_id for this connection
            {
                let mut current = peer_id.lock().await;
                // If this connection was already registered as a different peer, unregister old
                if let Some(ref old_id) = *current {
                    if old_id != &pid {
                        registry.unregister(old_id).await;
                        broadcaster.unregister_sender(old_id).await;
                    }
                }
                *current = Some(pid.clone());
            }

            broadcaster
                .register_sender(pid.clone(), tx.clone())
                .await;

            if is_new {
                let _ = tx.send(
                    SignalingMessage::Error {
                        message: format!("Registered as {}", pid),
                    }
                    .to_ws_message(),
                );
            } else {
                let _ = tx.send(
                    SignalingMessage::Error {
                        message: format!("Updated registration for {}", pid),
                    }
                    .to_ws_message(),
                );
            }
        }

        SignalingMessage::Lookup { target_peer_id } => {
            let response = match registry.lookup(&target_peer_id).await {
                Some(record) => SignalingMessage::LookupResponse {
                    target_peer_id: target_peer_id.clone(),
                    found: true,
                    address: Some(record.address.to_string()),
                    nat_type: Some(record.nat_type),
                },
                None => SignalingMessage::LookupResponse {
                    target_peer_id: target_peer_id.clone(),
                    found: false,
                    address: None,
                    nat_type: None,
                },
            };
            let _ = tx.send(response.to_ws_message());
        }

        SignalingMessage::ConnectRequest {
            from_peer_id,
            from_peer_name,
            target_peer_id,
            offer,
        } => {
            // Verify the requesting peer is registered
            if registry.lookup(&from_peer_id).await.is_none() {
                let _ = tx.send(
                    SignalingMessage::Error {
                        message: "You must register before connecting".into(),
                    }
                    .to_ws_message(),
                );
                return;
            }

            // Look up the target peer
            match registry.lookup(&target_peer_id).await {
                Some(_target_record) => {
                    let from_addr = match registry.lookup(&from_peer_id).await {
                        Some(r) => r.address.to_string(),
                        None => addr.to_string(),
                    };

                    // Forward the connection request to the target
                    let incoming = SignalingMessage::IncomingConnection {
                        from_peer_id: from_peer_id.clone(),
                        from_peer_name,
                        from_address: from_addr,
                        offer,
                    };

                    if !broadcaster.send_to(&target_peer_id, incoming).await {
                        let _ = tx.send(
                            SignalingMessage::Error {
                                message: format!("Cannot reach peer {}", target_peer_id),
                            }
                            .to_ws_message(),
                        );
                    }
                }
                None => {
                    let _ = tx.send(
                        SignalingMessage::Error {
                            message: format!("Peer {} not found", target_peer_id),
                        }
                        .to_ws_message(),
                    );
                }
            }
        }

        SignalingMessage::ConnectAccept {
            from_peer_id,
            target_peer_id,
            answer,
        } => {
            // Forward the acceptance to the requesting peer
            let accept = SignalingMessage::ConnectAccept {
                from_peer_id,
                target_peer_id: target_peer_id.clone(),
                answer,
            };
            broadcaster.send_to(&target_peer_id, accept).await;
        }

        SignalingMessage::ConnectReject {
            from_peer_id,
            target_peer_id,
            reason,
        } => {
            // Forward the rejection to the requesting peer
            let reject = SignalingMessage::ConnectReject {
                from_peer_id,
                target_peer_id: target_peer_id.clone(),
                reason,
            };
            broadcaster.send_to(&target_peer_id, reject).await;
        }

        SignalingMessage::RelayRequest {
            peer_id: pid,
            target_peer_id,
            session_id,
        } => {
            let sid = match session_id {
                Some(sid) => {
                    relay.register_connection(&sid, &pid).await.ok();
                    sid
                }
                None => {
                    // Create a new relay session
                    let sid = relay
                        .create_session(pid.clone(), target_peer_id.clone())
                        .await;
                    // Notify the target peer about the relay request
                    let relay_msg = SignalingMessage::RelayAssigned {
                        session_id: sid.clone(),
                        relay_port: 21119,
                    };
                    broadcaster.send_to(&target_peer_id, relay_msg).await;
                    sid
                }
            };

            let _ = tx.send(
                SignalingMessage::RelayAssigned {
                    session_id: sid,
                    relay_port: 21119,
                }
                .to_ws_message(),
            );
        }

        SignalingMessage::ListPeers => {
            let peers = registry.list_all().await;
            let entries: Vec<PeerListEntry> = peers
                .into_iter()
                .map(|p| PeerListEntry {
                    peer_id: p.peer_id,
                    peer_name: p.peer_name,
                    address: p.address.to_string(),
                    nat_type: p.nat_type,
                })
                .collect();
            let _ = tx.send(SignalingMessage::PeerList { peers: entries }.to_ws_message());
        }

        SignalingMessage::Heartbeat => {
            let pid_guard = peer_id.lock().await;
            if let Some(ref pid) = *pid_guard {
                registry.touch(pid).await;
            }
            let _ = tx.send(SignalingMessage::HeartbeatAck.to_ws_message());
        }

        SignalingMessage::IncomingConnection { .. }
        | SignalingMessage::LookupResponse { .. }
        | SignalingMessage::RelayAssigned { .. }
        | SignalingMessage::PeerList { .. }
        | SignalingMessage::HeartbeatAck
        | SignalingMessage::Error { .. } => {
            // These message types are server-to-client only
            let _ = tx.send(
                SignalingMessage::Error {
                    message: "Unexpected message type from client".into(),
                }
                .to_ws_message(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_message_valid_json() {
        let msg = Message::Text(r#"{"type":"heartbeat"}"#.to_string());
        assert!(validate_message(&msg).is_ok());
    }

    #[test]
    fn test_validate_message_too_large() {
        let big = "X".repeat(MAX_MESSAGE_SIZE + 1);
        let msg = Message::Text(format!(r#"{{"type":"heartbeat","data":"{}"}}"#, big));
        let _result = validate_message(&msg);
        // The message itself might not be > MAX_MESSAGE_SIZE due to JSON overhead,
        // so let's create one that definitely is
        let really_big = "X".repeat(MAX_MESSAGE_SIZE + 100);
        let msg = Message::Text(really_big);
        assert!(validate_message(&msg).is_err());
    }

    #[test]
    fn test_validate_message_invalid_json() {
        let msg = Message::Text("not json".to_string());
        assert!(validate_message(&msg).is_err());
    }

    #[test]
    fn test_validate_message_missing_type() {
        let msg = Message::Text(r#"{"data":"something"}"#.to_string());
        assert!(validate_message(&msg).is_err());
    }

    #[test]
    fn test_validate_message_unknown_type() {
        let msg = Message::Text(r#"{"type":"hack_attempt"}"#.to_string());
        assert!(validate_message(&msg).is_err());
    }

    #[test]
    fn test_validate_message_binary_rejected() {
        let msg = Message::Binary(vec![1, 2, 3]);
        assert!(validate_message(&msg).is_err());
    }

    #[test]
    fn test_validate_message_close_ok() {
        let msg = Message::Close(None);
        assert!(validate_message(&msg).is_ok());
    }

    #[test]
    fn test_rate_limiter_allows_burst() {
        let mut limiter = RateLimiter::new();
        // First MAX_MESSAGES_PER_SECOND messages should all be allowed
        for _ in 0..MAX_MESSAGES_PER_SECOND {
            assert!(limiter.check());
        }
    }

    #[test]
    fn test_signaling_broadcaster_send_to_nonexistent() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let broadcaster = SignalingBroadcaster::new();
            let msg = SignalingMessage::Heartbeat;
            let sent = broadcaster.send_to("nonexistent", msg).await;
            assert!(!sent);
        });
    }

    #[test]
    fn test_server_auth_no_auth_accepts_anything() {
        let auth = ServerAuth::no_auth();
        assert!(!auth.is_enabled());
        assert!(auth.verify(None));
        assert!(auth.verify(Some("random")));
    }

    #[test]
    fn test_server_auth_with_token_valid() {
        let auth = ServerAuth::with_token("secret123".to_string());
        assert!(auth.is_enabled());
        assert!(auth.verify(Some("secret123")));
    }

    #[test]
    fn test_server_auth_with_token_invalid() {
        let auth = ServerAuth::with_token("secret123".to_string());
        assert!(!auth.verify(None));
        assert!(!auth.verify(Some("wrong")));
        assert!(!auth.verify(Some("")));
    }
}
