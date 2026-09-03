use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time;

use alldesk_core::error::Error;
use alldesk_core::Result;

const DISCOVERY_PORT: u16 = 21117;
const BROADCAST_INTERVAL: Duration = Duration::from_secs(2);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDiscovered {
    pub peer_id: String,
    pub peer_name: String,
    pub addr: SocketAddr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveryMessage {
    peer_id: String,
    peer_name: String,
    port: u16,
}

/// Build a UDP socket with SO_REUSEADDR set, then convert to `tokio::net::UdpSocket`.
fn create_reuseaddr_udp_socket(bind_addr: &str) -> Result<tokio::net::UdpSocket> {
    let addr: SocketAddr = bind_addr
        .parse()
        .map_err(|e| Error::Network(format!("parse addr {}: {}", bind_addr, e)))?;
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| Error::Network(format!("create socket: {}", e)))?;
    socket
        .set_reuse_address(true)
        .map_err(|e| Error::Network(format!("set reuse_address: {}", e)))?;
    socket
        .bind(&addr.into())
        .map_err(|e| Error::Network(format!("bind {}: {}", bind_addr, e)))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| Error::Network(format!("set nonblocking: {}", e)))?;
    let std_socket: std::net::UdpSocket = socket.into();
    tokio::net::UdpSocket::from_std(std_socket)
        .map_err(|e| Error::Network(format!("from_std: {}", e)))
}

pub struct LanDiscovery {
    port: u16,
    socket: Option<tokio::net::UdpSocket>,
    running: bool,
}

impl LanDiscovery {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            socket: None,
            running: false,
        }
    }

    pub async fn start_broadcast(&mut self, peer_id: &str, peer_name: &str) -> Result<()> {
        let socket = create_reuseaddr_udp_socket("0.0.0.0:0")?;
        socket
            .set_broadcast(true)
            .map_err(|e| Error::Network(format!("set broadcast: {}", e)))?;

        let msg = DiscoveryMessage {
            peer_id: peer_id.to_string(),
            peer_name: peer_name.to_string(),
            port: self.port,
        };
        let data = serde_json::to_vec(&msg)
            .map_err(|e| Error::Network(format!("serialize discovery: {}", e)))?;

        let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT)
            .parse()
            .map_err(|e| Error::Network(format!("parse broadcast addr: {}", e)))?;

        socket
            .send_to(&data, broadcast_addr)
            .await
            .map_err(|e| Error::Network(format!("broadcast: {}", e)))?;

        self.socket = Some(socket);
        self.running = true;
        Ok(())
    }

    pub async fn discover(&self) -> Result<Vec<PeerDiscovered>> {
        let socket = create_reuseaddr_udp_socket(&format!("0.0.0.0:{}", DISCOVERY_PORT))?;

        let mut peers = Vec::new();
        let mut buf = [0u8; 4096];

        let deadline = tokio::time::Instant::now() + DISCOVERY_TIMEOUT;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
                Ok(Ok((len, addr))) => {
                    if let Ok(msg) = serde_json::from_slice::<DiscoveryMessage>(&buf[..len]) {
                        peers.push(PeerDiscovered {
                            peer_id: msg.peer_id,
                            peer_name: msg.peer_name,
                            addr: format!("{}:{}", addr.ip(), msg.port)
                                .parse()
                                .unwrap_or(addr),
                        });
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!("Discovery recv error: {}", e);
                    break;
                }
                Err(_) => {
                    break;
                }
            }
        }
        Ok(peers)
    }

    pub async fn discover_loop(
        peer_id: String,
        peer_name: String,
        port: u16,
        tx: tokio::sync::mpsc::Sender<PeerDiscovered>,
    ) -> Result<()> {
        let socket = create_reuseaddr_udp_socket(&format!("0.0.0.0:{}", DISCOVERY_PORT))?;
        socket
            .set_broadcast(true)
            .map_err(|e| Error::Network(format!("broadcast: {}", e)))?;

        let broadcast_msg = DiscoveryMessage {
            peer_id: peer_id.clone(),
            peer_name: peer_name.clone(),
            port,
        };
        let broadcast_data = serde_json::to_vec(&broadcast_msg)
            .map_err(|e| Error::Network(format!("serialize: {}", e)))?;
        let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT)
            .parse()
            .map_err(|e| Error::Network(format!("parse addr: {}", e)))?;

        let mut interval = time::interval(BROADCAST_INTERVAL);
        let mut buf = [0u8; 4096];

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = socket.send_to(&broadcast_data, broadcast_addr).await {
                        tracing::warn!("Discovery broadcast send error: {}", e);
                    }
                }
                result = socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, addr)) => {
                            if let Ok(msg) = serde_json::from_slice::<DiscoveryMessage>(&buf[..len]) {
                                if msg.peer_id != peer_id {
                                    let _ = tx.send(PeerDiscovered {
                                        peer_id: msg.peer_id,
                                        peer_name: msg.peer_name,
                                        addr: format!("{}:{}", addr.ip(), msg.port).parse()
                                            .unwrap_or(addr),
                                    }).await;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Discovery recv error: {}", e);
                        }
                    }
                }
            }
        }
    }

    pub fn stop(&mut self) -> Result<()> {
        self.running = false;
        self.socket = None;
        Ok(())
    }

    /// Test: try binding to the discovery port.
    pub fn test_socket() -> Result<()> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| Error::Network(format!("create socket: {}", e)))?;
        socket
            .set_reuse_address(true)
            .map_err(|e| Error::Network(format!("reuse_address: {}", e)))?;
        let addr: SocketAddr = format!("0.0.0.0:{}", DISCOVERY_PORT)
            .parse()
            .map_err(|e| Error::Network(format!("parse: {}", e)))?;
        socket
            .bind(&addr.into())
            .map_err(|e| Error::Network(format!("bind 0.0.0.0:{}: {}", DISCOVERY_PORT, e)))?;
        Ok(())
    }

    /// Test: try sending a broadcast packet.
    pub fn test_broadcast() -> Result<()> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| Error::Network(format!("create socket: {}", e)))?;
        socket
            .set_broadcast(true)
            .map_err(|e| Error::Network(format!("set_broadcast: {}", e)))?;
        let broadcast_addr: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT)
            .parse()
            .map_err(|e| Error::Network(format!("parse broadcast: {}", e)))?;
        socket
            .send_to(b"ALGDESK_PING".as_ref(), &broadcast_addr.into())
            .map_err(|e| Error::Network(format!("send_to broadcast: {}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_message_serde_roundtrip() {
        let msg = DiscoveryMessage {
            peer_id: "peer-123".to_string(),
            peer_name: "TestPeer".to_string(),
            port: 21116,
        };
        let json = serde_json::to_vec(&msg).unwrap();
        let decoded: DiscoveryMessage = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.peer_id, "peer-123");
        assert_eq!(decoded.peer_name, "TestPeer");
        assert_eq!(decoded.port, 21116);
    }

    #[test]
    fn test_peer_discovered_serde_roundtrip() {
        let peer = PeerDiscovered {
            peer_id: "abc".to_string(),
            peer_name: "Host1".to_string(),
            addr: "192.168.1.10:21116".parse().unwrap(),
        };
        let json = serde_json::to_string(&peer).unwrap();
        let decoded: PeerDiscovered = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.peer_id, "abc");
        assert_eq!(decoded.peer_name, "Host1");
        assert_eq!(
            decoded.addr,
            "192.168.1.10:21116".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn test_lan_discovery_new() {
        let discovery = LanDiscovery::new(21116);
        assert_eq!(discovery.port, 21116);
        assert!(discovery.socket.is_none());
        assert!(!discovery.running);
    }

    #[tokio::test]
    async fn test_discovery_loopback() {
        // Use a random high port for testing to avoid conflicts
        let server_port = 41117u16;
        let bind_addr = format!("0.0.0.0:{}", server_port);

        // Bind a listener on the discovery port
        let _socket = create_reuseaddr_udp_socket(&bind_addr).unwrap();

        // Create discovery instance
        let mut discovery = LanDiscovery::new(21116);

        // Start broadcast from a random port
        discovery
            .start_broadcast("test-peer-1", "TestHost")
            .await
            .unwrap();

        // The broadcast should have been sent
        assert!(discovery.running);
        assert!(discovery.socket.is_some());

        discovery.stop().unwrap();
        assert!(!discovery.running);
    }
}
