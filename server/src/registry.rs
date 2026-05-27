use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

/// Record for a registered peer in the signaling server.
#[derive(Debug, Clone)]
pub struct PeerRecord {
    pub peer_id: String,
    pub peer_name: String,
    pub address: SocketAddr,
    pub nat_type: NatType,
    pub last_seen: Instant,
}

/// Detected NAT type for a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NatType {
    /// No NAT detected (public IP).
    Public,
    /// Full cone NAT: any external host can send packets.
    FullCone,
    /// Restricted cone NAT: only hosts that received packets from internal host.
    RestrictedCone,
    /// Port restricted cone NAT: restricted to port.
    PortRestrictedCone,
    /// Symmetric NAT: different port mapping per destination.
    Symmetric,
    /// NAT type unknown / not yet detected.
    Unknown,
}

impl std::fmt::Display for NatType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NatType::Public => write!(f, "public"),
            NatType::FullCone => write!(f, "full_cone"),
            NatType::RestrictedCone => write!(f, "restricted_cone"),
            NatType::PortRestrictedCone => write!(f, "port_restricted_cone"),
            NatType::Symmetric => write!(f, "symmetric"),
            NatType::Unknown => write!(f, "unknown"),
        }
    }
}

/// How long before a peer is considered expired.
const PEER_TIMEOUT: Duration = Duration::from_secs(60);

/// Thread-safe peer registry.
#[derive(Debug, Clone)]
pub struct PeerRegistry {
    peers: Arc<Mutex<HashMap<String, PeerRecord>>>,
}

impl PeerRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register or update a peer. Returns true if this was a new registration.
    pub async fn register(&self, record: PeerRecord) -> bool {
        let is_new = !self.peers.lock().await.contains_key(&record.peer_id);
        if is_new {
            info!(
                "Peer registered: {} ({}) at {} [NAT: {}]",
                record.peer_id, record.peer_name, record.address, record.nat_type
            );
        } else {
            debug!(
                "Peer updated: {} ({}) at {} [NAT: {}]",
                record.peer_id, record.peer_name, record.address, record.nat_type
            );
        }
        let count = {
            let mut peers = self.peers.lock().await;
            peers.insert(record.peer_id.clone(), record);
            peers.len()
        };
        crate::metrics::record_active_peers(count);
        is_new
    }

    /// Remove a peer from the registry.
    pub async fn unregister(&self, peer_id: &str) -> bool {
        let mut peers = self.peers.lock().await;
        if let Some(record) = peers.remove(peer_id) {
            info!("Peer unregistered: {} ({})", record.peer_id, record.peer_name);
            crate::metrics::record_active_peers(peers.len());
            true
        } else {
            false
        }
    }

    /// Look up a peer by ID. Returns None if not found or expired.
    pub async fn lookup(&self, peer_id: &str) -> Option<PeerRecord> {
        let peers = self.peers.lock().await;
        peers.get(peer_id).and_then(|record| {
            if record.last_seen.elapsed() > PEER_TIMEOUT {
                warn!("Peer {} found but expired", peer_id);
                None
            } else {
                Some(record.clone())
            }
        })
    }

    /// List all non-expired peers.
    pub async fn list_all(&self) -> Vec<PeerRecord> {
        let peers = self.peers.lock().await;
        peers
            .values()
            .filter(|r| r.last_seen.elapsed() <= PEER_TIMEOUT)
            .cloned()
            .collect()
    }

    /// Remove all expired peers. Returns the number of removed peers.
    pub async fn cleanup_expired(&self) -> usize {
        let mut peers = self.peers.lock().await;
        let before = peers.len();
        peers.retain(|_, record| {
            let alive = record.last_seen.elapsed() <= PEER_TIMEOUT;
            if !alive {
                info!(
                    "Peer expired: {} ({})",
                    record.peer_id, record.peer_name
                );
            }
            alive
        });
        let removed = before - peers.len();
        crate::metrics::record_active_peers(peers.len());
        removed
    }

    /// Touch a peer's last-seen timestamp.
    pub async fn touch(&self, peer_id: &str) -> bool {
        let mut peers = self.peers.lock().await;
        if let Some(record) = peers.get_mut(peer_id) {
            record.last_seen = Instant::now();
            true
        } else {
            false
        }
    }

    /// Number of registered peers (including possibly expired ones).
    #[allow(dead_code)]
    pub async fn len(&self) -> usize {
        self.peers.lock().await.len()
    }

    /// Check if the registry is empty.
    #[allow(dead_code)]
    pub async fn is_empty(&self) -> bool {
        self.peers.lock().await.is_empty()
    }
}

/// Signaling messages exchanged over WebSocket between peers and the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalingMessage {
    /// Peer registers with the signaling server.
    Register {
        peer_id: String,
        peer_name: String,
        nat_type: NatType,
        /// Optional authentication token.
        auth_token: Option<String>,
    },
    /// Peer requests lookup of another peer.
    Lookup {
        target_peer_id: String,
    },
    /// Server responds with lookup result.
    LookupResponse {
        target_peer_id: String,
        found: bool,
        address: Option<String>,
        nat_type: Option<NatType>,
    },
    /// Peer requests to connect to another peer.
    ConnectRequest {
        from_peer_id: String,
        from_peer_name: String,
        target_peer_id: String,
        offer: Option<String>,
    },
    /// Target peer accepts connection.
    ConnectAccept {
        from_peer_id: String,
        target_peer_id: String,
        answer: Option<String>,
    },
    /// Target peer rejects connection.
    ConnectReject {
        from_peer_id: String,
        target_peer_id: String,
        reason: Option<String>,
    },
    /// Server notifies target peer of an incoming connection request.
    IncomingConnection {
        from_peer_id: String,
        from_peer_name: String,
        from_address: String,
        offer: Option<String>,
    },
    /// Request a relay session.
    RelayRequest {
        peer_id: String,
        target_peer_id: String,
        session_id: Option<String>,
    },
    /// Server assigns a relay session.
    RelayAssigned {
        session_id: String,
        relay_port: u16,
    },
    /// List all peers.
    ListPeers,
    /// Server responds with peer list.
    PeerList {
        peers: Vec<PeerListEntry>,
    },
    /// Heartbeat to keep registration alive.
    Heartbeat,
    /// Server acknowledges heartbeat.
    HeartbeatAck,
    /// Generic error from server.
    Error {
        message: String,
    },
}

/// Entry in the peer list sent to clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerListEntry {
    pub peer_id: String,
    pub peer_name: String,
    pub address: String,
    pub nat_type: NatType,
}

impl SignalingMessage {
    /// Return the message type name as a string (matches the serde tag).
    pub fn type_name(&self) -> &'static str {
        match self {
            SignalingMessage::Register { .. } => "register",
            SignalingMessage::Lookup { .. } => "lookup",
            SignalingMessage::LookupResponse { .. } => "lookup_response",
            SignalingMessage::ConnectRequest { .. } => "connect_request",
            SignalingMessage::ConnectAccept { .. } => "connect_accept",
            SignalingMessage::ConnectReject { .. } => "connect_reject",
            SignalingMessage::IncomingConnection { .. } => "incoming_connection",
            SignalingMessage::RelayRequest { .. } => "relay_request",
            SignalingMessage::RelayAssigned { .. } => "relay_assigned",
            SignalingMessage::ListPeers => "list_peers",
            SignalingMessage::PeerList { .. } => "peer_list",
            SignalingMessage::Heartbeat => "heartbeat",
            SignalingMessage::HeartbeatAck => "heartbeat_ack",
            SignalingMessage::Error { .. } => "error",
        }
    }

    /// Parse a signaling message from a WebSocket text message.
    pub fn from_ws_message(msg: &Message) -> anyhow::Result<Self> {
        match msg {
            Message::Text(text) => Ok(serde_json::from_str(text)?),
            Message::Close(_) => Err(anyhow::anyhow!("WebSocket closed")),
            _ => Err(anyhow::anyhow!("Unexpected message type")),
        }
    }

    /// Serialize to WebSocket text message.
    pub fn to_ws_message(&self) -> Message {
        Message::Text(serde_json::to_string(self).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(peer_id: &str) -> PeerRecord {
        PeerRecord {
            peer_id: peer_id.to_string(),
            peer_name: format!("Peer-{}", peer_id),
            address: "127.0.0.1:21116".parse().unwrap(),
            nat_type: NatType::Unknown,
            last_seen: Instant::now(),
        }
    }

    #[tokio::test]
    async fn test_registry_register_new() {
        let registry = PeerRegistry::new();
        let is_new = registry.register(make_record("p1")).await;
        assert!(is_new);
        assert!(!registry.is_empty().await);
    }

    #[tokio::test]
    async fn test_registry_register_update() {
        let registry = PeerRegistry::new();
        registry.register(make_record("p1")).await;

        let mut updated = make_record("p1");
        updated.peer_name = "Updated".to_string();
        let is_new = registry.register(updated).await;
        assert!(!is_new);

        let record = registry.lookup("p1").await.unwrap();
        assert_eq!(record.peer_name, "Updated");
    }

    #[tokio::test]
    async fn test_registry_unregister() {
        let registry = PeerRegistry::new();
        registry.register(make_record("p1")).await;

        let removed = registry.unregister("p1").await;
        assert!(removed);

        let removed_again = registry.unregister("p1").await;
        assert!(!removed_again);
    }

    #[tokio::test]
    async fn test_registry_lookup_not_found() {
        let registry = PeerRegistry::new();
        let result = registry.lookup("nonexistent").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_registry_list_all() {
        let registry = PeerRegistry::new();
        registry.register(make_record("p1")).await;
        registry.register(make_record("p2")).await;
        registry.register(make_record("p3")).await;

        let peers = registry.list_all().await;
        assert_eq!(peers.len(), 3);
    }

    #[tokio::test]
    async fn test_registry_touch() {
        let registry = PeerRegistry::new();
        registry.register(make_record("p1")).await;

        let touched = registry.touch("p1").await;
        assert!(touched);

        let not_touched = registry.touch("nonexistent").await;
        assert!(!not_touched);
    }

    #[tokio::test]
    async fn test_signaling_message_serde_roundtrip() {
        let messages = vec![
            SignalingMessage::Register {
                peer_id: "p1".into(),
                peer_name: "Test".into(),
                nat_type: NatType::FullCone,
                auth_token: None,
            },
            SignalingMessage::Lookup {
                target_peer_id: "p2".into(),
            },
            SignalingMessage::ConnectRequest {
                from_peer_id: "p1".into(),
                from_peer_name: "Test".into(),
                target_peer_id: "p2".into(),
                offer: Some("sdp-offer".into()),
            },
            SignalingMessage::Heartbeat,
            SignalingMessage::Error {
                message: "test error".into(),
            },
        ];

        for msg in messages {
            let ws_msg = msg.to_ws_message();
            let parsed = SignalingMessage::from_ws_message(&ws_msg).unwrap();

            let original_json = serde_json::to_string(&msg).unwrap();
            let parsed_json = serde_json::to_string(&parsed).unwrap();
            assert_eq!(original_json, parsed_json);
        }
    }

    #[test]
    fn test_nat_type_display() {
        assert_eq!(NatType::Public.to_string(), "public");
        assert_eq!(NatType::FullCone.to_string(), "full_cone");
        assert_eq!(NatType::Symmetric.to_string(), "symmetric");
        assert_eq!(NatType::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_nat_type_serde() {
        let json = serde_json::to_string(&NatType::PortRestrictedCone).unwrap();
        assert_eq!(json, "\"port_restricted_cone\"");
        let parsed: NatType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, NatType::PortRestrictedCone);
    }

    #[test]
    fn test_signaling_message_from_ws_close() {
        let msg = Message::Close(None);
        let result = SignalingMessage::from_ws_message(&msg);
        assert!(result.is_err());
    }

    #[test]
    fn test_signaling_message_from_ws_binary() {
        let msg = Message::Binary(vec![1, 2, 3]);
        let result = SignalingMessage::from_ws_message(&msg);
        assert!(result.is_err());
    }
}
