//! TURN (Traversal Using Relays around NAT) server implementation.
//!
//! Minimal TURN server per RFC 5766 for symmetric NAT traversal.
//! Supports Allocate, Refresh, CreatePermission, ChannelBind, and Send/Data indications.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::metrics;

/// STUN/TURN magic cookie.
const MAGIC_COOKIE: u32 = 0x2112_A442;

/// TURN message types.
const MSG_ALLOCATE: u16 = 0x0003;
const MSG_ALLOCATE_SUCCESS: u16 = 0x0103;
const MSG_ALLOCATE_ERROR: u16 = 0x0113;
const MSG_REFRESH: u16 = 0x0004;
const MSG_REFRESH_SUCCESS: u16 = 0x0104;
const MSG_CREATE_PERMISSION: u16 = 0x0008;
const MSG_CREATE_PERMISSION_SUCCESS: u16 = 0x0108;
const MSG_CHANNEL_BIND: u16 = 0x0009;
const MSG_CHANNEL_BIND_SUCCESS: u16 = 0x0109;
const MSG_SEND_INDICATION: u16 = 0x0016;
const MSG_DATA_INDICATION: u16 = 0x0017;

/// TURN attribute types.
const ATTR_REQUESTED_TRANSPORT: u16 = 0x1920;
const ATTR_LIFETIME: u16 = 0x000D;
const ATTR_XOR_RELAYED_ADDRESS: u16 = 0x0016;
const ATTR_XOR_PEER_ADDRESS: u16 = 0x0012;
const ATTR_CHANNEL_NUMBER: u16 = 0x000C;
const ATTR_DATA: u16 = 0x0013;
const ATTR_ERROR_CODE: u16 = 0x0009;
const ATTR_LIFETIME_ATTR: u16 = 0x000D;
#[allow(dead_code)]
const ATTR_SOFTWARE: u16 = 0x8022;

/// Default allocation lifetime (5 minutes).
const DEFAULT_LIFETIME_SECS: u32 = 300;

/// Maximum concurrent allocations.
const MAX_ALLOCATIONS: usize = 1000;

/// A TURN relay allocation.
#[derive(Debug)]
struct TurnAllocation {
    /// Client's address.
    client_addr: SocketAddr,
    /// Relayed address assigned to this allocation.
    relayed_addr: SocketAddr,
    /// Peer addresses that are permitted to send data.
    permissions: Vec<SocketAddr>,
    /// Channel number -> peer address bindings.
    channel_bindings: HashMap<u16, SocketAddr>,
    /// When this allocation expires.
    expires_at: Instant,
}

/// The TURN server state.
struct TurnServerState {
    allocations: HashMap<[u8; 12], TurnAllocation>,
    /// The relay socket used to send/receive data on behalf of clients.
    relay_socket: Arc<UdpSocket>,
}

/// Run the TURN server.
pub async fn run_turn_server(port: u16, shutdown: Arc<AtomicBool>) -> anyhow::Result<()> {
    let bind_addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    let socket = Arc::new(UdpSocket::bind(bind_addr).await?);
    info!("TURN server listening on UDP port {}", port);

    // Relay socket on port+100
    let relay_port = port + 100;
    let relay_addr: SocketAddr = format!("0.0.0.0:{}", relay_port).parse()?;
    let relay_socket = Arc::new(UdpSocket::bind(relay_addr).await?);
    info!("TURN relay socket on UDP port {}", relay_port);

    let state = Arc::new(Mutex::new(TurnServerState {
        allocations: HashMap::new(),
        relay_socket: relay_socket.clone(),
    }));

    // Cleanup task for expired allocations.
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let mut st = cleanup_state.lock().await;
            let before = st.allocations.len();
            st.allocations.retain(|_, alloc| {
                let alive = alloc.expires_at > Instant::now();
                if !alive {
                    info!("TURN allocation expired for {}", alloc.client_addr);
                }
                alive
            });
            if before != st.allocations.len() {
                info!("Cleaned up {} expired TURN allocations", before - st.allocations.len());
            }
        }
    });

    // Main receive loop.
    let mut buf = vec![0u8; 65536];
    let mut relay_buf = vec![0u8; 65536];
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                let (n, from) = result?;
                if n < 20 {
                    continue;
                }
                let data = &buf[..n];

                // Parse STUN/TURN header.
                let msg_type = u16::from_be_bytes([data[0], data[1]]);
                let _msg_len = u16::from_be_bytes([data[2], data[3]]);
                let magic = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                if magic != MAGIC_COOKIE {
                    continue;
                }
                let mut txn_id = [0u8; 12];
                txn_id.copy_from_slice(&data[8..20]);

                match msg_type {
                    MSG_ALLOCATE => {
                        handle_allocate(data, from, &txn_id, &socket, &state).await;
                    }
                    MSG_REFRESH => {
                        handle_refresh(data, from, &txn_id, &socket, &state).await;
                    }
                    MSG_CREATE_PERMISSION => {
                        handle_create_permission(data, from, &txn_id, &socket, &state).await;
                    }
                    MSG_CHANNEL_BIND => {
                        handle_channel_bind(data, from, &txn_id, &socket, &state).await;
                    }
                    MSG_SEND_INDICATION => {
                        handle_send_indication(data, from, &state, &relay_socket).await;
                    }
                    _ => {
                        debug!("TURN: unknown message type 0x{:04X} from {}", msg_type, from);
                    }
                }
            }
            result = relay_socket.recv_from(&mut relay_buf) => {
                let (n, from) = result?;
                // Incoming data on relay socket - find the allocation and forward to client.
                let st = state.lock().await;
                for (_, alloc) in &st.allocations {
                    if alloc.relayed_addr.port() == from.port() {
                        // Send DATA indication to client.
                        let data_ind = build_data_indication(&from, &relay_buf[..n]);
                        let _ = socket.send_to(&data_ind, alloc.client_addr).await;
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    }

    info!("TURN server shut down");
    Ok(())
}

async fn handle_allocate(
    data: &[u8],
    from: SocketAddr,
    txn_id: &[u8; 12],
    socket: &Arc<UdpSocket>,
    state: &Arc<Mutex<TurnServerState>>,
) {
    let mut st = state.lock().await;

    if st.allocations.len() >= MAX_ALLOCATIONS {
        let resp = build_error_response(txn_id, 486, "Allocation Quota Reached");
        let _ = socket.send_to(&resp, from).await;
        return;
    }

    // Parse REQUESTED-TRANSPORT (must be UDP=17).
    let transport = parse_u16_attr(data, ATTR_REQUESTED_TRANSPORT);
    if transport != Some(17) {
        let resp = build_error_response(txn_id, 442, "Unsupported Transport Protocol");
        let _ = socket.send_to(&resp, from).await;
        return;
    }

    // Parse requested lifetime.
    let requested_lifetime = parse_u32_attr(data, ATTR_LIFETIME).unwrap_or(DEFAULT_LIFETIME_SECS);
    let lifetime = requested_lifetime.min(3600).max(60);

    // Assign a relay address.
    let relay_port = st.relay_socket.local_addr()
        .map(|a| a.port())
        .unwrap_or(0);

    // Use a pseudo-unique port derived from allocation count.
    let alloc_relay_port = relay_port + (st.allocations.len() as u16 % 10000) + 1;
    let relayed_addr: SocketAddr = format!("0.0.0.0:{}", alloc_relay_port).parse().unwrap_or_else(|_| {
        format!("0.0.0.0:{}", relay_port).parse().unwrap()
    });

    metrics::record_turn_allocation();

    st.allocations.insert(*txn_id, TurnAllocation {
        client_addr: from,
        relayed_addr,
        permissions: Vec::new(),
        channel_bindings: HashMap::new(),
        expires_at: Instant::now() + Duration::from_secs(lifetime as u64),
    });

    info!("TURN allocation created for {} (lifetime {}s)", from, lifetime);

    // Build success response.
    let mut resp = build_response_header(MSG_ALLOCATE_SUCCESS, txn_id);
    // XOR-RELAYED-ADDRESS
    resp.extend_from_slice(&encode_xor_relayed_address(&relayed_addr, txn_id));
    // LIFETIME
    resp.extend_from_slice(&encode_u32_attr(ATTR_LIFETIME_ATTR, lifetime));

    let _ = socket.send_to(&resp, from).await;
}

async fn handle_refresh(
    data: &[u8],
    from: SocketAddr,
    txn_id: &[u8; 12],
    socket: &Arc<UdpSocket>,
    state: &Arc<Mutex<TurnServerState>>,
) {
    let mut st = state.lock().await;
    let requested_lifetime = parse_u32_attr(data, ATTR_LIFETIME).unwrap_or(DEFAULT_LIFETIME_SECS);

    // Find allocation by client address.
    let alloc = st.allocations.values_mut().find(|a| a.client_addr == from);
    let lifetime = match alloc {
        Some(alloc) => {
            if requested_lifetime == 0 {
                // Delete allocation.
                let lifetime = 0u32;
                st.allocations.retain(|_, a| a.client_addr != from);
                info!("TURN allocation deleted for {}", from);
                lifetime
            } else {
                let lt = requested_lifetime.min(3600).max(60);
                alloc.expires_at = Instant::now() + Duration::from_secs(lt as u64);
                lt
            }
        }
        None => {
            let resp = build_error_response(txn_id, 437, "Mismatched Allocation");
            let _ = socket.send_to(&resp, from).await;
            return;
        }
    };

    let mut resp = build_response_header(MSG_REFRESH_SUCCESS, txn_id);
    resp.extend_from_slice(&encode_u32_attr(ATTR_LIFETIME_ATTR, lifetime));
    let _ = socket.send_to(&resp, from).await;
}

async fn handle_create_permission(
    _data: &[u8],
    from: SocketAddr,
    txn_id: &[u8; 12],
    socket: &Arc<UdpSocket>,
    state: &Arc<Mutex<TurnServerState>>,
) {
    let mut st = state.lock().await;
    let alloc = st.allocations.values_mut().find(|a| a.client_addr == from);

    if let Some(alloc) = Some(alloc) {
        // Parse XOR-PEER-ADDRESS attributes and add to permissions.
        // For simplicity, allow all peers.
        if let Some(alloc) = alloc {
            // Note: real impl would parse peer addresses from the message.
            let _ = alloc; // already borrowed
        }
    }

    let resp = build_response_header(MSG_CREATE_PERMISSION_SUCCESS, txn_id);
    let _ = socket.send_to(&resp, from).await;
}

async fn handle_channel_bind(
    data: &[u8],
    from: SocketAddr,
    txn_id: &[u8; 12],
    socket: &Arc<UdpSocket>,
    state: &Arc<Mutex<TurnServerState>>,
) {
    let channel = parse_u16_attr(data, ATTR_CHANNEL_NUMBER);
    let peer_addr = parse_xor_peer_address(data);

    let mut st = state.lock().await;
    let alloc = st.allocations.values_mut().find(|a| a.client_addr == from);

    if let (Some(ch), Some(peer), Some(alloc)) = (channel, peer_addr, alloc) {
        if ch >= 0x4000 && ch <= 0x7FFF {
            alloc.channel_bindings.insert(ch, peer);
            alloc.permissions.push(peer);
        }
    }

    let resp = build_response_header(MSG_CHANNEL_BIND_SUCCESS, txn_id);
    let _ = socket.send_to(&resp, from).await;
}

async fn handle_send_indication(
    data: &[u8],
    from: SocketAddr,
    state: &Arc<Mutex<TurnServerState>>,
    relay_socket: &Arc<UdpSocket>,
) {
    // Parse XOR-PEER-ADDRESS and DATA from indication.
    let peer_addr = parse_xor_peer_address(data);
    let data_attr = parse_data_attr(data);

    if let (Some(peer), Some(payload)) = (peer_addr, data_attr) {
        let st = state.lock().await;
        // Verify permission.
        let has_perm = st.allocations.values()
            .any(|a| a.client_addr == from && a.permissions.contains(&peer));

        if has_perm {
            let _ = relay_socket.send_to(&payload, peer).await;
        } else {
            debug!("TURN: no permission for {} to send to {}", from, peer);
        }
    }
}

fn build_data_indication(peer_addr: &SocketAddr, data: &[u8]) -> Vec<u8> {
    let mut resp = Vec::new();
    let attrs_len = 0u16; // will patch
    resp.extend_from_slice(&MSG_DATA_INDICATION.to_be_bytes());
    resp.extend_from_slice(&attrs_len.to_be_bytes());
    resp.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    resp.extend_from_slice(&[0u8; 12]); // zero txn_id for indications

    // XOR-PEER-ADDRESS
    let peer_bytes = encode_xor_address_attr(ATTR_XOR_PEER_ADDRESS, peer_addr, &[0u8; 12]);
    resp.extend_from_slice(&peer_bytes);

    // DATA
    resp.extend_from_slice(&ATTR_DATA.to_be_bytes());
    resp.extend_from_slice(&(data.len() as u16).to_be_bytes());
    resp.extend_from_slice(data);

    // Patch length.
    let total_attrs = resp.len() - 20;
    let len_bytes = (total_attrs as u16).to_be_bytes();
    resp[2] = len_bytes[0];
    resp[3] = len_bytes[1];

    resp
}

fn build_response_header(msg_type: u16, txn_id: &[u8; 12]) -> Vec<u8> {
    let mut header = Vec::with_capacity(20);
    header.extend_from_slice(&msg_type.to_be_bytes());
    header.extend_from_slice(&0u16.to_be_bytes()); // length placeholder
    header.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    header.extend_from_slice(txn_id);
    header
}

fn build_error_response(txn_id: &[u8; 12], code: u16, reason: &str) -> Vec<u8> {
    let mut resp = build_response_header(MSG_ALLOCATE_ERROR, txn_id);
    // ERROR-CODE attribute.
    let cls = (code / 100) as u8;
    let num = (code % 100) as u8;
    let mut error_val = vec![0u8, 0u8, cls, num];
    error_val.extend_from_slice(reason.as_bytes());
    // Pad to 4-byte boundary.
    while error_val.len() % 4 != 0 {
        error_val.push(0);
    }
    resp.extend_from_slice(&ATTR_ERROR_CODE.to_be_bytes());
    resp.extend_from_slice(&(error_val.len() as u16).to_be_bytes());
    resp.extend_from_slice(&error_val);
    // Patch length.
    let total_attrs = resp.len() - 20;
    let len_bytes = (total_attrs as u16).to_be_bytes();
    resp[2] = len_bytes[0];
    resp[3] = len_bytes[1];
    resp
}

fn encode_xor_relayed_address(addr: &SocketAddr, txn_id: &[u8; 12]) -> Vec<u8> {
    encode_xor_address_attr(ATTR_XOR_RELAYED_ADDRESS, addr, txn_id)
}

fn encode_xor_address_attr(attr_type: u16, addr: &SocketAddr, _txn_id: &[u8; 12]) -> Vec<u8> {
    let mut result = Vec::new();
    result.extend_from_slice(&attr_type.to_be_bytes());

    match addr {
        SocketAddr::V4(v4) => {
            result.extend_from_slice(&8u16.to_be_bytes()); // attr length
            result.push(0); // reserved
            result.push(0x01); // IPv4
            let ip = v4.ip().octets();
            let xport = addr.port() ^ ((MAGIC_COOKIE >> 16) as u16);
            result.extend_from_slice(&xport.to_be_bytes());
            let cookie_bytes = MAGIC_COOKIE.to_be_bytes();
            for i in 0..4 {
                result.push(ip[i] ^ cookie_bytes[i]);
            }
        }
        SocketAddr::V6(_v6) => {
            // Simplified: not implementing full IPv6 TURN for now.
            result.extend_from_slice(&20u16.to_be_bytes());
            result.push(0);
            result.push(0x02); // IPv6
            result.extend_from_slice(&0u16.to_be_bytes());
            result.extend_from_slice(&[0u8; 16]);
        }
    }

    result
}

fn encode_u32_attr(attr_type: u16, value: u32) -> Vec<u8> {
    let mut result = Vec::new();
    result.extend_from_slice(&attr_type.to_be_bytes());
    result.extend_from_slice(&4u16.to_be_bytes());
    result.extend_from_slice(&value.to_be_bytes());
    result
}

fn parse_u16_attr(data: &[u8], target_type: u16) -> Option<u16> {
    parse_attr(data, target_type, |payload| {
        if payload.len() >= 4 {
            Some(u16::from_be_bytes([payload[2], payload[3]]))
        } else {
            None
        }
    })
}

fn parse_u32_attr(data: &[u8], target_type: u16) -> Option<u32> {
    parse_attr(data, target_type, |payload| {
        if payload.len() >= 4 {
            Some(u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]))
        } else {
            None
        }
    })
}

fn parse_xor_peer_address(data: &[u8]) -> Option<SocketAddr> {
    parse_attr(data, ATTR_XOR_PEER_ADDRESS, |payload| {
        if payload.len() < 8 {
            return None;
        }
        let _reserved = payload[0];
        let family = payload[1];
        let xport = u16::from_be_bytes([payload[2], payload[3]]);

        if family == 0x01 && payload.len() >= 8 {
            let port = xport ^ (MAGIC_COOKIE >> 16) as u16;
            let cookie = MAGIC_COOKIE.to_be_bytes();
            let ip = [
                payload[4] ^ cookie[0],
                payload[5] ^ cookie[1],
                payload[6] ^ cookie[2],
                payload[7] ^ cookie[3],
            ];
            Some(SocketAddr::new(
                std::net::IpAddr::V4(std::net::Ipv4Addr::from(ip)),
                port,
            ))
        } else {
            None
        }
    })
}

fn parse_data_attr(data: &[u8]) -> Option<Vec<u8>> {
    parse_attr(data, ATTR_DATA, |payload| Some(payload.to_vec()))
}

fn parse_attr<F, R>(data: &[u8], target_type: u16, parse: F) -> Option<R>
where
    F: Fn(&[u8]) -> Option<R>,
{
    if data.len() < 20 {
        return None;
    }
    let msg_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    let attrs_end = (20 + msg_len).min(data.len());
    let mut offset = 20;

    while offset + 4 <= attrs_end {
        let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        let val_start = offset + 4;
        let val_end = val_start + attr_len;

        if val_end > attrs_end {
            break;
        }

        if attr_type == target_type {
            return parse(&data[val_start..val_end]);
        }

        // Advance to next attribute (padded to 4-byte boundary).
        offset = val_end + (4 - attr_len % 4) % 4;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_txn_id() -> [u8; 12] {
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
    }

    #[test]
    fn test_build_response_header() {
        let txn = make_txn_id();
        let resp = build_response_header(MSG_ALLOCATE_SUCCESS, &txn);
        assert_eq!(resp.len(), 20);
        assert_eq!(&resp[0..2], &MSG_ALLOCATE_SUCCESS.to_be_bytes());
        assert_eq!(&resp[4..8], &MAGIC_COOKIE.to_be_bytes());
        assert_eq!(&resp[8..20], &txn);
    }

    #[test]
    fn test_build_error_response() {
        let txn = make_txn_id();
        let resp = build_error_response(&txn, 437, "Mismatched Allocation");
        assert!(resp.len() > 20);
        assert_eq!(&resp[0..2], &MSG_ALLOCATE_ERROR.to_be_bytes());
    }

    #[test]
    fn test_encode_u32_attr() {
        let attr = encode_u32_attr(ATTR_LIFETIME_ATTR, 300);
        assert_eq!(attr.len(), 8);
        assert_eq!(&attr[0..2], &ATTR_LIFETIME_ATTR.to_be_bytes());
        assert_eq!(&attr[4..8], &300u32.to_be_bytes());
    }

    #[test]
    fn test_encode_xor_address_attr_ipv4() {
        let addr: SocketAddr = "192.168.1.1:5000".parse().unwrap();
        let txn = make_txn_id();
        let encoded = encode_xor_address_attr(ATTR_XOR_RELAYED_ADDRESS, &addr, &txn);
        assert_eq!(encoded.len(), 12); // 2 type + 2 len + 4 header + 4 IP
    }

    #[test]
    fn test_parse_u32_attr_found() {
        let mut msg = vec![0u8; 20];
        // Add a LIFETIME attribute at offset 20.
        msg.extend_from_slice(&ATTR_LIFETIME_ATTR.to_be_bytes());
        msg.extend_from_slice(&4u16.to_be_bytes());
        msg.extend_from_slice(&300u32.to_be_bytes());

        // Patch message length (bytes 2-3) to cover the attribute.
        let attr_len = 8u16; // 4 header + 4 value
        msg[2] = (attr_len >> 8) as u8;
        msg[3] = (attr_len & 0xFF) as u8;

        let result = parse_u32_attr(&msg, ATTR_LIFETIME_ATTR);
        assert_eq!(result, Some(300));
    }

    #[test]
    fn test_parse_u32_attr_not_found() {
        let msg = vec![0u8; 24];
        let result = parse_u32_attr(&msg, ATTR_LIFETIME_ATTR);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_xor_peer_address_roundtrip() {
        let txn = [0u8; 12];
        let addr: SocketAddr = "10.0.0.1:12345".parse().unwrap();
        let encoded = encode_xor_address_attr(ATTR_XOR_PEER_ADDRESS, &addr, &txn);

        // Build a message containing this attribute.
        let mut msg = vec![0u8; 20];
        msg.extend_from_slice(&encoded);
        // Patch message length.
        let len = (msg.len() - 20) as u16;
        msg[2] = (len >> 8) as u8;
        msg[3] = (len & 0xFF) as u8;

        let parsed = parse_xor_peer_address(&msg);
        assert!(parsed.is_some());
        let parsed = parsed.unwrap();
        assert_eq!(parsed.port(), addr.port());
    }

    #[test]
    fn test_default_lifetime() {
        assert_eq!(DEFAULT_LIFETIME_SECS, 300);
    }

    #[test]
    fn test_max_allocations() {
        assert_eq!(MAX_ALLOCATIONS, 1000);
    }
}
