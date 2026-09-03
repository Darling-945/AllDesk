use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

#[cfg(test)]
use std::net::Ipv6Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tracing::{debug, error, info, warn};

use crate::registry::NatType;

/// STUN magic cookie value (RFC 5389).
const STUN_MAGIC_COOKIE: u32 = 0x2112_A442;

/// STUN Binding Request type.
const STUN_BINDING_REQUEST: u16 = 0x0001;
/// STUN Binding Response type.
const STUN_BINDING_RESPONSE: u16 = 0x0101;

/// XOR-MAPPED-ADDRESS attribute type.
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
/// MAPPED-ADDRESS attribute type.
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
/// SOFTWARE attribute type.
const ATTR_SOFTWARE: u16 = 0x8022;
/// FINGERPRINT attribute type.
#[allow(dead_code)]
const ATTR_FINGERPRINT: u16 = 0x8028;

/// IPv4 family indicator.
const FAMILY_IPV4: u8 = 0x01;
/// IPv6 family indicator.
const FAMILY_IPV6: u8 = 0x02;

/// Run the STUN server on the given UDP port.
///
/// Uses a single async recv loop with `tokio::net::UdpSocket` and dispatches
/// based on message type. NAT classification requests are also answered from
/// an alternate port (port+1) when available, without competing for the same
/// socket.
pub async fn run_stun_server(port: u16, shutdown: Arc<AtomicBool>) -> anyhow::Result<()> {
    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
    let socket = UdpSocket::bind(addr).await?;
    info!("STUN server listening on UDP port {}", port);

    // Bind alternate socket for NAT classification responses (different port).
    let alt_port = port + 1;
    let alt_socket = match UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, alt_port)).await {
        Ok(s) => {
            info!("STUN NAT detection alternate socket on UDP port {}", alt_port);
            Some(Arc::new(s))
        }
        Err(e) => {
            warn!("Could not bind alternate STUN port {}: {}", alt_port, e);
            None
        }
    };

    let mut buf = [0u8; 576]; // STUN messages are typically small

    loop {
        if shutdown.load(Ordering::Relaxed) {
            info!("STUN server shutting down gracefully");
            break;
        }

        tokio::select! {
            recv_result = socket.recv_from(&mut buf) => {
                let (len, src) = match recv_result {
                    Ok(result) => result,
                    Err(e) => {
                        error!("STUN recv error: {}", e);
                        continue;
                    }
                };

                debug!("STUN received {} bytes from {}", len, src);

                let data = &buf[..len];

                // Handle standard STUN Binding Requests
                if is_stun_binding_request(data) {
                    crate::metrics::record_stun_request();
                    if let Some(response) = handle_stun_request(data, src) {
                        // Respond from the main port
                        if let Err(e) = socket.send_to(&response, src).await {
                            warn!("STUN send error to {}: {}", src, e);
                        } else {
                            debug!("STUN sent binding response to {}", src);
                        }

                        // For NAT classification requests, also respond from the alternate port.
                        // The client can compare the source port of both responses to determine
                        // if its NAT mapping is symmetric.
                        if is_nat_classification_request(data) {
                            debug!("NAT classification request from {}", src);
                            if let Some(ref alt) = alt_socket {
                                if let Some(response) = handle_stun_request(data, src) {
                                    if let Err(e) = alt.send_to(&response, src).await {
                                        warn!("STUN alt send error to {}: {}", src, e);
                                    } else {
                                        debug!("STUN sent alternate binding response to {}", src);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Check if a STUN message is a Binding Request.
fn is_stun_binding_request(data: &[u8]) -> bool {
    if data.len() < 20 {
        return false;
    }
    let msg_type = u16::from_be_bytes([data[0], data[1]]);
    let magic = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

    msg_type == STUN_BINDING_REQUEST && magic == STUN_MAGIC_COOKIE
}

/// Check if this is a NAT classification request (uses a custom attribute marker).
fn is_nat_classification_request(data: &[u8]) -> bool {
    if !is_stun_binding_request(data) {
        return false;
    }
    // Check if the message contains a custom attribute 0x8000 (NAT classification marker)
    if data.len() < 24 {
        return false;
    }
    let attr_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    let attrs_start = 20;
    let attrs_end = attrs_start + attr_len;
    if attrs_end > data.len() {
        return false;
    }

    let mut pos = attrs_start;
    while pos + 4 <= attrs_end {
        let attr_type = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let attr_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        if attr_type == 0x8000 {
            return true;
        }
        pos += 4 + attr_len;
        // Align to 4-byte boundary
        pos = (pos + 3) & !3;
    }
    false
}

/// Handle an incoming STUN request, producing a Binding Response.
fn handle_stun_request(data: &[u8], src: SocketAddr) -> Option<Vec<u8>> {
    if !is_stun_binding_request(data) {
        debug!("Non-STUN-binding message received, ignoring");
        return None;
    }

    if data.len() < 20 {
        return None;
    }

    // Extract the transaction ID from the request (bytes 8..20)
    let transaction_id = &data[8..20];

    // Build the response
    build_binding_response(transaction_id, src)
}

/// Build a STUN Binding Response with the XOR-MAPPED-ADDRESS attribute.
fn build_binding_response(transaction_id: &[u8], mapped_addr: SocketAddr) -> Option<Vec<u8>> {
    let software = b"AllDesk STUN Server";
    let mut attrs = Vec::new();

    // XOR-MAPPED-ADDRESS attribute
    let xor_addr = encode_xor_mapped_address(mapped_addr, transaction_id);
    attrs.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
    attrs.extend_from_slice(&(xor_addr.len() as u16).to_be_bytes());
    attrs.extend_from_slice(&xor_addr);

    // MAPPED-ADDRESS attribute (for older clients)
    let plain_addr = encode_mapped_address(mapped_addr);
    attrs.extend_from_slice(&ATTR_MAPPED_ADDRESS.to_be_bytes());
    attrs.extend_from_slice(&(plain_addr.len() as u16).to_be_bytes());
    attrs.extend_from_slice(&plain_addr);

    // SOFTWARE attribute
    attrs.extend_from_slice(&ATTR_SOFTWARE.to_be_bytes());
    let padded_len = ((software.len() + 3) & !3) as u16;
    attrs.extend_from_slice(&padded_len.to_be_bytes());
    attrs.extend_from_slice(software);
    // Pad to 4-byte boundary
    let padding = padded_len as usize - software.len();
    attrs.extend(std::iter::repeat_n(0u8, padding));

    // Calculate total message length (header is 20 bytes)
    let msg_len = attrs.len() as u16;

    // Build message header
    let mut response = Vec::with_capacity(20 + attrs.len());
    response.extend_from_slice(&STUN_BINDING_RESPONSE.to_be_bytes()); // Type
    response.extend_from_slice(&msg_len.to_be_bytes()); // Length
    response.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes()); // Magic cookie
    response.extend_from_slice(transaction_id); // Transaction ID
    response.extend_from_slice(&attrs); // Attributes

    Some(response)
}

/// Encode a MAPPED-ADDRESS attribute (no XOR).
fn encode_mapped_address(addr: SocketAddr) -> Vec<u8> {
    match addr {
        SocketAddr::V4(v4) => {
            let mut result = Vec::with_capacity(8);
            result.push(0x00); // Reserved
            result.push(FAMILY_IPV4);
            result.extend_from_slice(&v4.port().to_be_bytes());
            result.extend_from_slice(&v4.ip().octets());
            result
        }
        SocketAddr::V6(v6) => {
            let mut result = Vec::with_capacity(20);
            result.push(0x00); // Reserved
            result.push(FAMILY_IPV6);
            result.extend_from_slice(&v6.port().to_be_bytes());
            result.extend_from_slice(&v6.ip().octets());
            result
        }
    }
}

/// Encode a XOR-MAPPED-ADDRESS attribute (RFC 5389).
fn encode_xor_mapped_address(addr: SocketAddr, transaction_id: &[u8]) -> Vec<u8> {
    match addr {
        SocketAddr::V4(v4) => {
            let mut result = Vec::with_capacity(8);
            result.push(0x00); // Reserved
            result.push(FAMILY_IPV4);

            // XOR port with the top 16 bits of the magic cookie
            let xored_port = v4.port() ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
            result.extend_from_slice(&xored_port.to_be_bytes());

            // XOR IP with the magic cookie
            let ip_bytes = v4.ip().octets();
            let cookie_bytes = STUN_MAGIC_COOKIE.to_be_bytes();
            result.extend_from_slice(&[
                ip_bytes[0] ^ cookie_bytes[0],
                ip_bytes[1] ^ cookie_bytes[1],
                ip_bytes[2] ^ cookie_bytes[2],
                ip_bytes[3] ^ cookie_bytes[3],
            ]);
            result
        }
        SocketAddr::V6(v6) => {
            let mut result = Vec::with_capacity(20);
            result.push(0x00); // Reserved
            result.push(FAMILY_IPV6);

            // XOR port with the top 16 bits of the magic cookie
            let xored_port = v6.port() ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
            result.extend_from_slice(&xored_port.to_be_bytes());

            // XOR IP with magic cookie (4 bytes) + transaction ID (12 bytes) = 16 bytes
            let ip_bytes = v6.ip().octets();
            let cookie_bytes = STUN_MAGIC_COOKIE.to_be_bytes();
            // First 4 bytes XOR with magic cookie
            result.extend_from_slice(&[
                ip_bytes[0] ^ cookie_bytes[0],
                ip_bytes[1] ^ cookie_bytes[1],
                ip_bytes[2] ^ cookie_bytes[2],
                ip_bytes[3] ^ cookie_bytes[3],
            ]);
            // Remaining 12 bytes XOR with transaction ID
            for i in 0..12 {
                result.push(ip_bytes[4 + i] ^ transaction_id[i]);
            }
            result
        }
    }
}

/// Determine NAT type based on observations from STUN binding tests.
/// This is a simplified implementation of RFC 3489 NAT detection.
#[allow(dead_code)]
pub fn detect_nat_type(
    local_addr: SocketAddr,
    public_addr: SocketAddr,
    same_port_response: bool,
    alt_port_response: bool,
) -> NatType {
    // If local address equals public address, there is no NAT
    if local_addr == public_addr {
        return NatType::Public;
    }

    // If the peer receives different addresses from different STUN ports,
    // it is a symmetric NAT
    if !alt_port_response {
        return NatType::Symmetric;
    }

    // If both ports responded, we can differentiate cone types
    if same_port_response {
        // Full cone NAT: receives from same port
        NatType::FullCone
    } else {
        // Port restricted or restricted cone
        NatType::PortRestrictedCone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stun_binding_request_detection() {
        let mut msg = [0u8; 28];
        msg[0..2].copy_from_slice(&STUN_BINDING_REQUEST.to_be_bytes());
        msg[2..4].copy_from_slice(&8u16.to_be_bytes()); // attribute length
        msg[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        // Fill transaction ID with non-zero bytes
        for i in 8..20 {
            msg[i] = 0x42;
        }
        assert!(is_stun_binding_request(&msg));

        // Too short
        assert!(!is_stun_binding_request(&msg[..10]));

        // Wrong magic
        msg[4] = 0x00;
        assert!(!is_stun_binding_request(&msg));
    }

    #[test]
    fn test_build_binding_response() {
        let transaction_id = [0x42u8; 12];
        let addr = SocketAddr::new(
            std::net::IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5)),
            12345,
        );

        let response = build_binding_response(&transaction_id, addr).unwrap();
        // Check message type is Binding Response
        assert_eq!(u16::from_be_bytes([response[0], response[1]]), STUN_BINDING_RESPONSE);
        // Check magic cookie
        assert_eq!(
            u32::from_be_bytes([response[4], response[5], response[6], response[7]]),
            STUN_MAGIC_COOKIE
        );
        // Check transaction ID
        assert_eq!(&response[8..20], &[0x42u8; 12]);
    }

    #[test]
    fn test_nat_type_detection() {
        let local = SocketAddr::new(
            std::net::IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
            12345,
        );
        let public = SocketAddr::new(
            std::net::IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5)),
            12345,
        );

        // Same address = public
        assert_eq!(
            detect_nat_type(local, local, true, true),
            NatType::Public
        );

        // Symmetric NAT
        assert_eq!(
            detect_nat_type(local, public, true, false),
            NatType::Symmetric
        );

        // Full cone
        assert_eq!(
            detect_nat_type(local, public, true, true),
            NatType::FullCone
        );
    }

    #[test]
    fn test_encode_mapped_address_ipv6() {
        let addr = SocketAddr::new(
            std::net::IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
            12345,
        );
        let encoded = encode_mapped_address(addr);
        assert_eq!(encoded[0], 0x00); // Reserved
        assert_eq!(encoded[1], FAMILY_IPV6);
        assert_eq!(u16::from_be_bytes([encoded[2], encoded[3]]), 12345);
        assert_eq!(encoded.len(), 20);
    }

    #[test]
    fn test_encode_xor_mapped_address_ipv6() {
        let transaction_id = [0x42u8; 12];
        let addr = SocketAddr::new(
            std::net::IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
            12345,
        );
        let encoded = encode_xor_mapped_address(addr, &transaction_id);
        assert_eq!(encoded[0], 0x00); // Reserved
        assert_eq!(encoded[1], FAMILY_IPV6);
        // Port should be XORed with top 16 bits of magic cookie
        let xored_port = u16::from_be_bytes([encoded[2], encoded[3]]);
        assert_eq!(xored_port, 12345 ^ ((STUN_MAGIC_COOKIE >> 16) as u16));
        assert_eq!(encoded.len(), 20);
    }

    #[test]
    fn test_build_binding_response_ipv6() {
        let transaction_id = [0x42u8; 12];
        let addr = SocketAddr::new(
            std::net::IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
            12345,
        );

        let response = build_binding_response(&transaction_id, addr).unwrap();
        assert_eq!(u16::from_be_bytes([response[0], response[1]]), STUN_BINDING_RESPONSE);
        assert_eq!(
            u32::from_be_bytes([response[4], response[5], response[6], response[7]]),
            STUN_MAGIC_COOKIE
        );
        assert_eq!(&response[8..20], &[0x42u8; 12]);
    }
}
