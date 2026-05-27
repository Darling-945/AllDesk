pub mod channel;
pub mod discovery;
pub mod e2e_crypto;
pub mod flow;
pub mod ice;
pub mod latency;
pub mod quic_conn;
pub mod reconnect;
pub mod transport;

#[allow(dead_code)]
pub mod bwe;

use async_trait::async_trait;
use alldesk_core::Result;

pub use channel::Channel;
pub use discovery::{LanDiscovery, PeerDiscovered};
pub use flow::{FlowController, FlowConfig, ChannelStats};
pub use ice::{IceAgent, IceCandidate, IceCandidateType};
pub use quic_conn::{QuicEndpoint, cert_fingerprint};
pub use reconnect::{ReconnectManager, ConnectionState};
pub use transport::QuicTransport;
pub use latency::{StageTimer, PipelineLatencyTracker, LatencyStats, LatencySample};
pub use e2e_crypto::{E2ECrypto, CryptoAlgorithm, hmac_verify, hmac_check};

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&mut self, channel: Channel, data: &[u8]) -> Result<()>;
    async fn recv(&mut self, channel: Channel) -> Result<Vec<u8>>;
    async fn close(&mut self) -> Result<()>;
    fn is_p2p(&self) -> bool;
    fn rtt_ms(&self) -> f64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_quic_endpoint_server_client_connect() {
        let server = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).unwrap();
        let _addr = server.local_addr().unwrap();

        let client = QuicEndpoint::new_client().unwrap();

        let server_clone = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).unwrap();
        // Verify we can create endpoints
        assert!(server.local_addr().is_ok());
        assert!(client.local_addr().is_ok());

        server.close();
        server_clone.close();
    }

    #[tokio::test]
    async fn test_quic_loopback_send_recv() {
        let server = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr().unwrap();

        let client = QuicEndpoint::new_client().unwrap();

        // Accept and connect concurrently
        let server_handle = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            let transport = QuicTransport::new(conn, true);
            transport
        });

        let client_conn = client.connect(addr).await.unwrap();
        let mut client_transport = QuicTransport::new(client_conn, true);

        let mut server_transport = server_handle.await.unwrap();

        // Accept the stream that the client will open
        let server_accept = tokio::spawn(async move {
            let ch = server_transport.accept_stream().await.unwrap();
            let data = server_transport.recv(ch).await.unwrap();
            (server_transport, ch, data)
        });

        // Client sends on the Input channel (reliable stream)
        client_transport.send(Channel::Input, b"hello world").await.unwrap();

        let (mut server_transport, _ch, data) = server_accept.await.unwrap();
        assert_eq!(&data, b"hello world");

        // Server responds
        server_transport.send(Channel::Input, b"ack").await.unwrap();
        let response = client_transport.recv(Channel::Input).await.unwrap();
        assert_eq!(&response, b"ack");

        // Verify properties
        assert!(server_transport.is_p2p());
        assert!(server_transport.rtt_ms() >= 0.0);

        server_transport.close().await.unwrap();
        client_transport.close().await.unwrap();
    }
}
