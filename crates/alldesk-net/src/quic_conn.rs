use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use quinn::{ClientConfig, Connection, Endpoint, ServerConfig, TransportConfig, VarInt};
use rcgen::{CertificateParams, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use alldesk_core::error::Error;
use alldesk_core::Result;

const ALPN: &[&[u8]] = &[b"alldesk"];

/// Ensure rustls CryptoProvider is installed. Called once at module init.
fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Compute the SHA-256 fingerprint of a DER-encoded certificate.
/// Returns a hex string like "AA:BB:CC:DD:...".
pub fn cert_fingerprint(cert_der: &[u8]) -> String {
    use std::fmt::Write;
    let hash = sha256(cert_der);
    hash.iter()
        .fold(String::with_capacity(hash.len() * 3 - 1), |mut acc, b| {
            if !acc.is_empty() {
                acc.write_char(':').unwrap();
            }
            write!(acc, "{:02X}", b).unwrap();
            acc
        })
}

/// Simple SHA-256 implementation for certificate fingerprinting.
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = ring::digest::Context::new(&ring::digest::SHA256);
    hasher.update(data);
    let result = hasher.finish();
    let mut out = [0u8; 32];
    out.copy_from_slice(result.as_ref());
    out
}

fn generate_self_signed_cert() -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let params = CertificateParams::new(Vec::<String>::new())
        .map_err(|e| Error::Network(format!("cert params: {}", e)))?;
    let key_pair =
        KeyPair::generate().map_err(|e| Error::Network(format!("generate key: {}", e)))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| Error::Network(format!("self sign: {}", e)))?;
    let cert_der = CertificateDer::from(cert.der().to_vec());
    let key_der = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
    Ok((vec![cert_der], key_der))
}

/// Build a low-latency TransportConfig tuned for remote desktop streaming.
/// Key optimizations:
/// - Minimal initial RTT estimate (5ms for LAN)
/// - Faster loss detection thresholds
/// - Larger datagram receive buffer for video frames
/// - Keep-alive for quick dead-connection detection
fn low_latency_transport_config() -> TransportConfig {
    let mut config = TransportConfig::default();
    // Aggressive initial RTT — LAN remote desktop targets <5ms RTT.
    config.initial_rtt(Duration::from_millis(5));
    // Larger datagram receive buffer for video/audio frames.
    config.datagram_receive_buffer_size(Some(65536));
    // Larger datagram send buffer to allow burst of video frames.
    config.datagram_send_buffer_size(65536);
    // Keep-alive to detect dead connections quickly.
    config.keep_alive_interval(Some(Duration::from_secs(2)));
    // Allow spin bit for RTT estimation.
    config.allow_spin(true);
    // Faster loss detection: lower packet threshold (default 3 → 2).
    config.packet_threshold(2);
    // Faster time threshold for loss detection (default 1.25 → 1.125).
    config.time_threshold(1.125);
    // Persistent congestion threshold — recover faster from sustained loss.
    config.persistent_congestion_threshold(2);
    // Stream receive window large enough for raw 4K frame fallback (~32MB).
    config.stream_receive_window(VarInt::from_u32(16 * 1024 * 1024));
    // Connection-level receive window.
    config.receive_window(VarInt::from_u32(32 * 1024 * 1024));
    // Send window for streams (must accommodate raw frame fallback).
    config.send_window(16 * 1024 * 1024);
    config
}

fn make_server_config() -> Result<ServerConfig> {
    ensure_crypto_provider();
    let (certs, key) = generate_self_signed_cert()?;
    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| Error::Network(format!("tls server config: {}", e)))?;
    tls_config.alpn_protocols = ALPN.iter().map(|p| p.to_vec()).collect();
    let quic_server_config = quinn::crypto::rustls::QuicServerConfig::try_from(tls_config)
        .map_err(|e| Error::Network(format!("quic server config: {}", e)))?;
    let mut server_config = ServerConfig::with_crypto(Arc::new(quic_server_config));
    server_config.transport_config(Arc::new(low_latency_transport_config()));
    Ok(server_config)
}

/// Create a client config that accepts any certificate (legacy LAN mode).
fn make_client_config_insecure() -> Result<ClientConfig> {
    ensure_crypto_provider();
    let mut tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier::insecure()))
        .with_no_client_auth();
    tls_config.alpn_protocols = ALPN.iter().map(|p| p.to_vec()).collect();
    let mut client_config = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls_config)
            .map_err(|e| Error::Network(format!("quic client config: {}", e)))?,
    ));
    client_config.transport_config(Arc::new(low_latency_transport_config()));
    Ok(client_config)
}

/// Create a client config that pins to specific certificate fingerprints.
fn make_client_config_pinned(fingerprints: Vec<String>) -> Result<ClientConfig> {
    ensure_crypto_provider();
    let mut tls_config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier::pinned(fingerprints)))
        .with_no_client_auth();
    tls_config.alpn_protocols = ALPN.iter().map(|p| p.to_vec()).collect();
    let mut client_config = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls_config)
            .map_err(|e| Error::Network(format!("quic client config: {}", e)))?,
    ));
    client_config.transport_config(Arc::new(low_latency_transport_config()));
    Ok(client_config)
}

fn make_client_config() -> Result<ClientConfig> {
    make_client_config_insecure()
}

/// Certificate verifier supporting both insecure (LAN) and pinned fingerprint modes.
#[derive(Debug)]
struct PinVerifier {
    /// If empty, accept any certificate (insecure/LAN mode).
    /// If non-empty, only accept certificates whose SHA-256 fingerprint matches one in the list.
    allowed_fingerprints: Vec<String>,
}

impl PinVerifier {
    fn insecure() -> Self {
        Self {
            allowed_fingerprints: Vec::new(),
        }
    }

    fn pinned(fingerprints: Vec<String>) -> Self {
        Self {
            allowed_fingerprints: fingerprints,
        }
    }
}

impl rustls::client::danger::ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if self.allowed_fingerprints.is_empty() {
            // Insecure mode: accept any certificate (LAN-only).
            return Ok(rustls::client::danger::ServerCertVerified::assertion());
        }

        // Pinned mode: check fingerprint against allowlist.
        let fp = cert_fingerprint(end_entity.as_ref());
        if self
            .allowed_fingerprints
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(&fp))
        {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(format!(
                "certificate fingerprint mismatch: got {}",
                fp
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

#[derive(Clone)]
pub struct QuicEndpoint {
    endpoint: Endpoint,
}

impl QuicEndpoint {
    pub fn new_server(bind_addr: SocketAddr) -> Result<Self> {
        let server_config = make_server_config()?;
        let endpoint = Endpoint::server(server_config, bind_addr)
            .map_err(|e| Error::Network(format!("create server endpoint: {}", e)))?;
        Ok(Self { endpoint })
    }

    pub fn new_client() -> Result<Self> {
        let bind_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let client_config = make_client_config()?;
        let mut endpoint = Endpoint::client(bind_addr)
            .map_err(|e| Error::Network(format!("create client endpoint: {}", e)))?;
        endpoint.set_default_client_config(client_config);
        Ok(Self { endpoint })
    }

    /// Create a client endpoint that pins to specific certificate fingerprints.
    pub fn new_client_pinned(fingerprints: Vec<String>) -> Result<Self> {
        let bind_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let client_config = make_client_config_pinned(fingerprints)?;
        let mut endpoint = Endpoint::client(bind_addr)
            .map_err(|e| Error::Network(format!("create client endpoint: {}", e)))?;
        endpoint.set_default_client_config(client_config);
        Ok(Self { endpoint })
    }

    pub async fn connect(&self, addr: SocketAddr) -> Result<Connection> {
        let conn = self
            .endpoint
            .connect(addr, "alldesk")
            .map_err(|e| Error::Network(format!("initiate connection: {}", e)))?
            .await
            .map_err(|e| Error::Network(format!("connect: {}", e)))?;
        Ok(conn)
    }

    pub async fn accept(&self) -> Result<Connection> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or_else(|| Error::Network("endpoint closed".into()))?;
        let conn = incoming
            .await
            .map_err(|e| Error::Network(format!("accept: {}", e)))?;
        Ok(conn)
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.endpoint
            .local_addr()
            .map_err(|e| Error::Network(format!("local addr: {}", e)))
    }

    pub fn close(&self) {
        self.endpoint.close(0u32.into(), b"shutdown");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Channel, QuicTransport, Transport};
    use rustls::client::danger::ServerCertVerifier;

    #[test]
    fn test_cert_fingerprint_format() {
        let fp = cert_fingerprint(&[0x01, 0x02, 0xAB, 0xCD]);
        assert!(fp.contains(':'));
        // Each byte becomes 2 hex chars + colon separator.
        let parts: Vec<&str> = fp.split(':').collect();
        assert_eq!(parts.len(), 32); // SHA-256 = 32 bytes
    }

    #[test]
    fn test_cert_fingerprint_deterministic() {
        let data = vec![42u8; 100];
        let fp1 = cert_fingerprint(&data);
        let fp2 = cert_fingerprint(&data);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_cert_fingerprint_different_inputs() {
        let fp1 = cert_fingerprint(&[1, 2, 3]);
        let fp2 = cert_fingerprint(&[4, 5, 6]);
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_sha256_known_vector() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let hash = sha256(&[]);
        assert_eq!(hash[0], 0xe3);
        assert_eq!(hash[31], 0x55);
    }

    #[test]
    fn test_pin_verifier_insecure_accepts_anything() {
        let verifier = PinVerifier::insecure();
        let cert = CertificateDer::from(vec![1, 2, 3]);
        let result = verifier.verify_server_cert(
            &cert,
            &[],
            &rustls::pki_types::ServerName::try_from("test").unwrap(),
            &[],
            rustls::pki_types::UnixTime::now(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_pin_verifier_pinned_accepts_matching() {
        let cert_data = vec![42u8; 50];
        let fp = cert_fingerprint(&cert_data);
        let verifier = PinVerifier::pinned(vec![fp]);
        let cert = CertificateDer::from(cert_data);
        let result = verifier.verify_server_cert(
            &cert,
            &[],
            &rustls::pki_types::ServerName::try_from("test").unwrap(),
            &[],
            rustls::pki_types::UnixTime::now(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_pin_verifier_pinned_rejects_non_matching() {
        let cert_data = vec![42u8; 50];
        let wrong_fp = cert_fingerprint(&[99u8; 50]);
        let verifier = PinVerifier::pinned(vec![wrong_fp]);
        let cert = CertificateDer::from(cert_data);
        let result = verifier.verify_server_cert(
            &cert,
            &[],
            &rustls::pki_types::ServerName::try_from("test").unwrap(),
            &[],
            rustls::pki_types::UnixTime::now(),
        );
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_low_latency_config_connectivity() {
        // Verify that low-latency transport config doesn't break connectivity.
        let server = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr().unwrap();
        let client = QuicEndpoint::new_client().unwrap();

        let server_handle = tokio::spawn(async move {
            let conn = server.accept().await.unwrap();
            QuicTransport::new(conn, true)
        });

        let client_conn = client.connect(addr).await.unwrap();
        let mut client_transport = QuicTransport::new(client_conn, true);
        let mut server_transport = server_handle.await.unwrap();

        // Verify data flows correctly with the low-latency config.
        let server_task = tokio::spawn(async move {
            let ch = server_transport.accept_stream().await.unwrap();
            let data = server_transport.recv(ch).await.unwrap();
            (server_transport, data)
        });

        client_transport
            .send(Channel::Input, b"low-latency-test")
            .await
            .unwrap();
        let (_, data) = server_task.await.unwrap();
        assert_eq!(&data, b"low-latency-test");

        client_transport.close().await.unwrap();
    }
}
