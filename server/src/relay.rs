use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::metrics;
use crate::registry::PeerRegistry;
use crate::signaling::SignalingBroadcaster;

/// A relay session that bridges two QUIC connections.
#[derive(Debug)]
#[allow(dead_code)]
pub struct RelaySession {
    pub session_id: String,
    pub peer_a_id: String,
    pub peer_b_id: String,
    /// Last time data was forwarded through this session.
    pub last_active: std::time::Instant,
    /// Bandwidth rate limiter for this session.
    pub bandwidth_limiter: Arc<tokio::sync::Mutex<TokenBucket>>,
}

/// How long an active relay session can be idle before cleanup (5 minutes).
const SESSION_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Default bandwidth limit per relay session in bytes per second (50 MB/s).
const DEFAULT_SESSION_BANDWIDTH: u64 = 50 * 1024 * 1024;

/// Token bucket rate limiter for bandwidth control per session.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TokenBucket {
    /// Maximum tokens (bytes) the bucket can hold.
    max_tokens: u64,
    /// Current number of tokens available.
    tokens: u64,
    /// Refill rate in tokens (bytes) per second.
    refill_rate: u64,
    /// Last time the bucket was refilled.
    last_refill: std::time::Instant,
}

impl TokenBucket {
    /// Create a new token bucket with the given rate (bytes/sec) and burst capacity.
    pub fn new(rate_bytes_per_sec: u64, burst: u64) -> Self {
        Self {
            max_tokens: burst,
            tokens: burst,
            refill_rate: rate_bytes_per_sec,
            last_refill: std::time::Instant::now(),
        }
    }

    /// Try to consume `count` tokens. Returns how many can be consumed immediately.
    #[allow(dead_code)]
    pub fn try_consume(&mut self, count: u64) -> u64 {
        self.refill();
        let available = self.tokens.min(count);
        self.tokens -= available;
        available
    }

    /// Refill tokens based on elapsed time.
    fn refill(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let tokens_to_add = (elapsed.as_secs_f64() * self.refill_rate as f64) as u64;
        if tokens_to_add > 0 {
            self.tokens = (self.tokens + tokens_to_add).min(self.max_tokens);
            self.last_refill = now;
        }
    }

    /// Get current available tokens.
    #[allow(dead_code)]
    pub fn available(&self) -> u64 {
        self.tokens
    }
}

/// The relay server manages QUIC relay sessions.
#[derive(Debug, Clone)]
pub struct RelayServer {
    sessions: Arc<Mutex<HashMap<String, RelaySession>>>,
    pending: Arc<Mutex<HashMap<String, PendingRelay>>>,
    #[allow(dead_code)]
    registry: PeerRegistry,
    #[allow(dead_code)]
    broadcaster: SignalingBroadcaster,
    #[allow(dead_code)]
    base_port: u16,
}

/// A peer waiting for its relay partner to connect.
#[derive(Debug)]
struct PendingRelay {
    session_id: String,
    peer_id: String,
    #[allow(dead_code)]
    target_peer_id: String,
    connection: quinn::Connection,
    created_at: std::time::Instant,
}

impl RelayServer {
    /// Create a new relay server.
    pub fn new(
        registry: PeerRegistry,
        broadcaster: SignalingBroadcaster,
        base_port: u16,
    ) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            registry,
            broadcaster,
            base_port,
        }
    }

    /// Start the QUIC relay listener on the given port.
    pub async fn start(&self, port: u16) -> anyhow::Result<()> {
        let endpoint = create_relay_endpoint(port)?;
        info!("Relay server QUIC endpoint listening on port {}", port);

        // Spawn cleanup task for expired pending connections
        let cleanup_pending = self.pending.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                let mut pending = cleanup_pending.lock().await;
                let before = pending.len();
                pending.retain(|_, p| {
                    let alive = p.created_at.elapsed() < std::time::Duration::from_secs(120);
                    if !alive {
                        info!(
                            "Pending relay {} expired for peer {}",
                            p.session_id, p.peer_id
                        );
                    }
                    alive
                });
                if before != pending.len() {
                    debug!(
                        "Cleaned up {} expired pending relays",
                        before - pending.len()
                    );
                }
            }
        });

        // Periodically log stats and clean up idle sessions
        let stats_sessions = self.sessions.clone();
        let stats_pending = self.pending.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let mut sessions = stats_sessions.lock().await;
                let before = sessions.len();
                sessions.retain(|id, session| {
                    let alive = session.last_active.elapsed() < SESSION_IDLE_TIMEOUT;
                    if !alive {
                        info!(
                            "Relay session {} expired (idle > {:?}, peers: {} <-> {})",
                            id, SESSION_IDLE_TIMEOUT, session.peer_a_id, session.peer_b_id
                        );
                    }
                    alive
                });
                let expired = before - sessions.len();
                if expired > 0 {
                    info!("Cleaned up {} idle relay sessions", expired);
                }
                let active = sessions.len();
                metrics::record_active_relay_sessions(active);
                drop(sessions);
                let pending = stats_pending.lock().await.len();
                info!("Relay stats: {} active sessions, {} pending", active, pending);
            }
        });

        // Accept QUIC connections
        info!("Relay server accepting connections...");
        while let Some(incoming) = endpoint.accept().await {
            let conn = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Relay QUIC accept error: {}", e);
                    continue;
                }
            };

            let remote = conn.remote_address();
            info!("Relay QUIC connection from {}", remote);

            let pending = self.pending.clone();
            let sessions = self.sessions.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_relay_connection(conn, pending, sessions).await {
                    error!("Relay connection error: {}", e);
                }
            });
        }

        Ok(())
    }

    /// Create a new relay session for two peers. Returns the session ID.
    pub async fn create_session(
        &self,
        peer_a_id: String,
        peer_b_id: String,
    ) -> String {
        let session_id = uuid::Uuid::new_v4().to_string();
        info!(
            "Created relay session {} for peers {} <-> {}",
            session_id, peer_a_id, peer_b_id
        );
        metrics::record_relay_session();
        session_id
    }

    /// Register a peer's QUIC connection for an existing relay session.
    pub async fn register_connection(
        &self,
        session_id: &str,
        peer_id: &str,
    ) -> anyhow::Result<()> {
        // This is a placeholder for signaling-triggerled relay setup.
        // Actual connection matching happens in handle_relay_connection.
        debug!(
            "Relay register_connection: session={}, peer={}",
            session_id, peer_id
        );
        Ok(())
    }

    /// Get the number of active relay sessions.
    #[allow(dead_code)]
    pub async fn active_session_count(&self) -> usize {
        self.sessions.lock().await.len()
    }

    /// Get the number of pending relay connections.
    #[allow(dead_code)]
    pub async fn pending_count(&self) -> usize {
        self.pending.lock().await.len()
    }
}

/// Handle an incoming relay QUIC connection.
async fn handle_relay_connection(
    conn: quinn::Connection,
    pending: Arc<Mutex<HashMap<String, PendingRelay>>>,
    sessions: Arc<Mutex<HashMap<String, RelaySession>>>,
) -> anyhow::Result<()> {
    // The first bidirectional stream is the control channel.
    // The peer sends a JSON message identifying the session.
    let (mut send, mut recv) = conn
        .accept_bi()
        .await
        .map_err(|e| anyhow::anyhow!("accept_bi: {}", e))?;

    // Read the control message
    let mut buf = vec![0u8; 4096];
    let n = recv
        .read(&mut buf)
        .await
        .map_err(|e| anyhow::anyhow!("read control: {}", e))?
        .ok_or_else(|| anyhow::anyhow!("empty control message"))?;

    let control: RelayControlMessage = serde_json::from_slice(&buf[..n])
        .map_err(|e| anyhow::anyhow!("parse control: {}", e))?;

    debug!(
        "Relay control: session_id={}, peer_id={}",
        control.session_id, control.peer_id
    );

    // Acknowledge
    let ack = serde_json::to_vec(&RelayControlAck {
        status: "ok".to_string(),
    })
    .unwrap_or_default();
    send.write_all(&ack)
        .await
        .map_err(|e| anyhow::anyhow!("send ack: {}", e))?;
    let _ = send.finish();

    // Check if partner is already waiting
    let mut pending_guard = pending.lock().await;
    if let Some(partner) = pending_guard.values().find(|p| {
        p.session_id == control.session_id && p.peer_id != control.peer_id
    }) {
        let partner_conn = partner.connection.clone();
        let partner_peer_id = partner.peer_id.clone();
        let session_id = control.session_id.clone();
        let peer_id = control.peer_id.clone();

        pending_guard.retain(|_, p| {
            !(p.session_id == control.session_id && p.peer_id != control.peer_id)
        });
        drop(pending_guard);

        info!(
            "Relay session {} now bridging {} <-> {}",
            session_id, peer_id, partner_peer_id
        );
        tokio::spawn(async move {
            bridge_connections(
                &session_id,
                peer_id,
                conn,
                &partner_peer_id,
                partner_conn,
                sessions,
            )
            .await;
        });
    } else {
        // Store pending
        pending_guard.insert(
            control.peer_id.clone(),
            PendingRelay {
                session_id: control.session_id.clone(),
                peer_id: control.peer_id.clone(),
                target_peer_id: String::new(),
                connection: conn,
                created_at: std::time::Instant::now(),
            },
        );
    }

    Ok(())
}

/// Bridge data between two QUIC connections in a relay session.
async fn bridge_connections(
    session_id: &str,
    peer_a_id: String,
    conn_a: quinn::Connection,
    peer_b_id: &str,
    conn_b: quinn::Connection,
    sessions: Arc<Mutex<HashMap<String, RelaySession>>>,
) {
    let sid = session_id.to_string();
    info!("Starting relay bridge for session {}", sid);

    let now = std::time::Instant::now();

    let bw_limiter = Arc::new(tokio::sync::Mutex::new(
        TokenBucket::new(DEFAULT_SESSION_BANDWIDTH, DEFAULT_SESSION_BANDWIDTH),
    ));

    // Register the active session
    {
        let mut ss = sessions.lock().await;
        ss.insert(
            sid.clone(),
            RelaySession {
                session_id: sid.clone(),
                peer_a_id: peer_a_id.clone(),
                peer_b_id: peer_b_id.to_string(),
                last_active: now,
                bandwidth_limiter: bw_limiter.clone(),
            },
        );
        metrics::record_active_relay_sessions(ss.len());
    }

    // Forward data in both directions. Clone the connections since
    // quinn::Connection is Arc-based and cheap to clone.
    let conn_a_clone = conn_a.clone();
    let conn_b_clone = conn_b.clone();

    let result: Result<(), anyhow::Error> = tokio::select! {
        r = forward_all_streams(&sid, "A->B", conn_a, conn_b_clone) => {
            info!("A->B forwarding ended for session {}", sid);
            r
        }
        r = forward_all_streams(&sid, "B->A", conn_b, conn_a_clone) => {
            info!("B->A forwarding ended for session {}", sid);
            r
        }
    };

    if let Err(e) = result {
        warn!("Relay session {} ended with error: {}", sid, e);
    } else {
        info!("Relay session {} ended cleanly", sid);
    }

    // Clean up session
    let mut ss = sessions.lock().await;
    ss.remove(&sid);
    metrics::record_active_relay_sessions(ss.len());
}

/// Forward all streams/datagrams from one connection to another.
async fn forward_all_streams(
    session_id: &str,
    direction: &str,
    from: quinn::Connection,
    to: quinn::Connection,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            // Forward bidirectional streams
            result = from.accept_bi() => {
                let (send, recv) = result
                    .map_err(|e| anyhow::anyhow!("{} accept_bi: {}", direction, e))?;

                let to_conn = to.clone();
                let dir = direction.to_string();
                let sid = session_id.to_string();
                tokio::spawn(async move {
                    if let Err(e) = forward_bi_stream(&sid, &dir, recv, send, to_conn).await {
                        debug!("{} stream forwarding error: {}", dir, e);
                    }
                });
            }
            // Forward datagrams (unreliable, for video/audio)
            result = from.read_datagram() => {
                let data = result
                    .map_err(|e| anyhow::anyhow!("{} read_datagram: {}", direction, e))?;
                to.send_datagram(data)
                    .map_err(|e| anyhow::anyhow!("{} send_datagram: {}", direction, e))?;
            }
        }
    }
}

/// Forward a single bidirectional stream to the peer.
///
/// Opens a matching bidirectional stream on the target connection and relays
/// data in both directions: received data is forwarded to the target's send
/// side, and data coming back from the target's recv side is forwarded to
/// the original peer via `original_send`.
async fn forward_bi_stream(
    _session_id: &str,
    _direction: &str,
    mut recv: quinn::RecvStream,
    mut original_send: quinn::SendStream,
    to: quinn::Connection,
) -> anyhow::Result<()> {
    // Open a corresponding bidirectional stream on the target connection.
    let (mut target_send, mut target_recv) = to.open_bi().await
        .map_err(|e| anyhow::anyhow!("open_bi to target: {}", e))?;

    // Forward data from recv -> target_send (original peer to target).
    let copy_forward = tokio::io::copy(&mut recv, &mut target_send);

    // Forward data from target_recv -> original_send (target back to original peer).
    let copy_backward = tokio::io::copy(&mut target_recv, &mut original_send);

    // Run both directions concurrently.
    tokio::select! {
        r = copy_forward => {
            r.map_err(|e| anyhow::anyhow!("stream copy forward: {}", e))?;
        }
        r = copy_backward => {
            r.map_err(|e| anyhow::anyhow!("stream copy backward: {}", e))?;
        }
    }

    let _ = target_send.finish();
    Ok(())
}

/// Control message sent by a peer when connecting to the relay.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RelayControlMessage {
    session_id: String,
    peer_id: String,
}

/// Acknowledgment of the control message.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RelayControlAck {
    status: String,
}

/// Create a Quinn QUIC endpoint for the relay server.
fn create_relay_endpoint(port: u16) -> anyhow::Result<quinn::Endpoint> {
    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
    let cert = generate_self_signed_cert()?;

    let server_config =
        quinn::ServerConfig::with_single_cert(vec![cert.cert_der], cert.key_der)?;

    let endpoint = quinn::Endpoint::server(server_config, addr.into())?;
    Ok(endpoint)
}

/// A self-signed certificate for the relay server.
struct RelayCert {
    #[allow(dead_code)]
    cert: rcgen::Certificate,
    cert_der: rustls::pki_types::CertificateDer<'static>,
    key_der: rustls::pki_types::PrivateKeyDer<'static>,
}

fn generate_self_signed_cert() -> anyhow::Result<RelayCert> {
    let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let rcgen::CertifiedKey { cert, key_pair } =
        rcgen::generate_simple_self_signed(subject_alt_names)?;

    let cert_der = cert.der().clone();
    let key_der =
        rustls::pki_types::PrivateKeyDer::Pkcs8(key_pair.serialize_der().into());

    Ok(RelayCert {
        cert,
        cert_der,
        key_der,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_new() {
        let tb = TokenBucket::new(1000, 500);
        assert_eq!(tb.max_tokens, 500);
        assert_eq!(tb.available(), 500);
    }

    #[test]
    fn test_token_bucket_consume_within_capacity() {
        let mut tb = TokenBucket::new(1000, 1000);
        let consumed = tb.try_consume(500);
        assert_eq!(consumed, 500);
        assert_eq!(tb.available(), 500);
    }

    #[test]
    fn test_token_bucket_consume_over_capacity() {
        let mut tb = TokenBucket::new(1000, 100);
        let consumed = tb.try_consume(500);
        assert_eq!(consumed, 100);
        assert_eq!(tb.available(), 0);
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut tb = TokenBucket::new(1000, 1000);
        tb.try_consume(500);
        assert_eq!(tb.available(), 500);
        // Wait a bit for refill
        std::thread::sleep(std::time::Duration::from_millis(50));
        tb.refill();
        assert!(tb.available() > 500);
    }

    #[test]
    fn test_token_bucket_refill_caps_at_max() {
        let mut tb = TokenBucket::new(1000, 100);
        tb.tokens = 99;
        std::thread::sleep(std::time::Duration::from_millis(10));
        tb.refill();
        assert!(tb.available() <= 100);
    }

    #[test]
    fn test_token_bucket_consume_zero() {
        let mut tb = TokenBucket::new(1000, 100);
        tb.try_consume(100);
        let consumed = tb.try_consume(1);
        assert_eq!(consumed, 0);
    }
}
