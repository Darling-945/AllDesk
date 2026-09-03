//! Integration tests for AllDesk QUIC transport, discovery, ICE, flow control,
//! and reconnect functionality.

use alldesk_net::*;
use std::collections::HashSet;
use std::time::Duration;

/// Helper: create a server+client pair connected over loopback.
/// Returns (server_transport, client_transport).
async fn setup_loopback() -> (QuicTransport, QuicTransport) {
    let server = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = server.local_addr().unwrap();
    let client = QuicEndpoint::new_client().unwrap();

    let server_handle = tokio::spawn(async move {
        let conn = server.accept().await.unwrap();
        QuicTransport::new(conn, true)
    });

    let client_conn = client.connect(addr).await.unwrap();
    let client_transport = QuicTransport::new(client_conn, true);
    let server_transport = server_handle.await.unwrap();

    (server_transport, client_transport)
}

// ---------------------------------------------------------------------------
// Test 1: Multiple channels over a single QUIC connection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_quic_loopback_multiple_channels() {
    let (server_transport, mut client_transport) = setup_loopback().await;

    let channels_and_payloads: Vec<(Channel, Vec<u8>)> = vec![
        (Channel::Video, b"video-frame-data".to_vec()),
        (Channel::Input, b"mouse-move-100-200".to_vec()),
        (Channel::Clipboard, b"clipboard-text-content".to_vec()),
    ];
    let num_channels = channels_and_payloads.len();

    // Server: accept one stream per channel, then receive.
    let server_task = tokio::spawn(async move {
        let mut st = server_transport;
        let mut results: Vec<(Channel, Vec<u8>)> = Vec::new();
        for _ in 0..num_channels {
            let ch = st.accept_stream().await.unwrap();
            let data = st.recv(ch).await.unwrap();
            results.push((ch, data));
        }
        results
    });

    // Client sends on each channel.
    for (ch, data) in &channels_and_payloads {
        client_transport.send(*ch, data).await.unwrap();
    }

    let results = server_task.await.unwrap();

    assert_eq!(results.len(), channels_and_payloads.len());
    for (i, (expected_ch, expected_data)) in channels_and_payloads.iter().enumerate() {
        assert_eq!(results[i].0, *expected_ch, "channel mismatch at index {}", i);
        assert_eq!(&results[i].1, expected_data, "data mismatch at index {}", i);
    }

    // Also verify Audio (datagram) can be sent without error.
    client_transport.send(Channel::Audio, b"opus-packet").await.unwrap();
}

// ---------------------------------------------------------------------------
// Test 2: 1 MB message over QUIC
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_quic_loopback_large_message() {
    let (server_transport, mut client_transport) = setup_loopback().await;

    // 1 MB payload with a repeating pattern.
    let large_data: Vec<u8> = (0..251u8).cycle().take(1024 * 1024).collect();
    assert_eq!(large_data.len(), 1024 * 1024);

    let server_task = tokio::spawn(async move {
        let mut st = server_transport;
        let ch = st.accept_stream().await.unwrap();
        st.recv(ch).await.unwrap()
    });

    client_transport.send(Channel::Input, &large_data).await.unwrap();

    let received = server_task.await.unwrap();

    assert_eq!(received.len(), large_data.len(), "length mismatch");
    for (i, (a, b)) in large_data.iter().zip(received.iter()).enumerate() {
        assert_eq!(a, b, "byte mismatch at index {}", i);
    }
}

// ---------------------------------------------------------------------------
// Test 3: Bidirectional messaging
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_quic_loopback_bidirectional() {
    let (server_transport, mut client_transport) = setup_loopback().await;

    let client_msg = b"hello from client".to_vec();
    let server_msg = b"greetings from server".to_vec();

    // Server: accept client's Input stream, recv, then send on Clipboard.
    // Keep the server transport alive until the client has finished reading.
    let server_msg_clone = server_msg.clone();
    let server_task = tokio::spawn(async move {
        let mut st = server_transport;

        // Accept client's stream (Input channel).
        let ch = st.accept_stream().await.unwrap();
        let from_client = st.recv(ch).await.unwrap();

        // Server sends on Clipboard channel.
        st.send(Channel::Clipboard, &server_msg_clone).await.unwrap();

        // Wait for client to accept and read our stream before dropping.
        // Hold the transport alive by sleeping briefly to let the client
        // accept the stream we just opened.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        from_client
    });

    // Client sends on Input, then accepts server's Clipboard stream concurrently.
    client_transport.send(Channel::Input, &client_msg).await.unwrap();

    // Client accepts server's stream (Clipboard) and reads.
    let client_task = tokio::spawn(async move {
        let ch = client_transport.accept_stream().await.unwrap();
        client_transport.recv(ch).await.unwrap()
    });

    // Await both tasks concurrently so neither transport is dropped prematurely.
    let (server_result, client_result) = tokio::join!(server_task, client_task);
    let from_client = server_result.unwrap();
    let from_server = client_result.unwrap();

    assert_eq!(&from_client, &client_msg);
    assert_eq!(&from_server, &server_msg);
}

// ---------------------------------------------------------------------------
// Test 4: Discovery message serialize / deserialize round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_discovery_message_roundtrip() {
    let peer = PeerDiscovered {
        peer_id: "test-peer-42".to_string(),
        peer_name: "TestHost-AllDesk".to_string(),
        addr: "192.168.1.100:21116".parse().unwrap(),
    };

    let json = serde_json::to_vec(&peer).unwrap();
    assert!(!json.is_empty());

    let decoded: PeerDiscovered = serde_json::from_slice(&json).unwrap();

    assert_eq!(decoded.peer_id, peer.peer_id);
    assert_eq!(decoded.peer_name, peer.peer_name);
    assert_eq!(decoded.addr, peer.addr);

    // Verify distinct peers produce distinct serializations.
    let peer2 = PeerDiscovered {
        peer_id: "other-peer-99".to_string(),
        peer_name: "OtherHost".to_string(),
        addr: "10.0.0.5:21116".parse().unwrap(),
    };
    let json2 = serde_json::to_vec(&peer2).unwrap();
    assert_ne!(json, json2);

    let decoded2: PeerDiscovered = serde_json::from_slice(&json2).unwrap();
    assert_ne!(decoded2.peer_id, decoded.peer_id);
}

// ---------------------------------------------------------------------------
// Test 5: ICE candidate exchange and pair formation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ice_candidate_exchange() {
    let mut agent_a = IceAgent::new();
    let mut agent_b = IceAgent::new();

    // Gather host candidates from local interfaces.
    let a_cands = agent_a.gather_host_candidates(21116).unwrap();
    let b_cands = agent_b.gather_host_candidates(21116).unwrap();

    assert!(!a_cands.is_empty(), "agent A should gather at least one host candidate");
    assert!(!b_cands.is_empty(), "agent B should gather at least one host candidate");

    for c in &a_cands {
        assert_eq!(c.candidate_type, IceCandidateType::Host);
    }

    // Add server-reflexive candidates to simulate STUN binding.
    let a_srflx = agent_a.add_server_reflexive("203.0.113.10:21116".parse().unwrap());
    assert_eq!(a_srflx.candidate_type, IceCandidateType::ServerReflexive);

    let b_srflx = agent_b.add_server_reflexive("203.0.113.20:21116".parse().unwrap());
    assert_eq!(b_srflx.candidate_type, IceCandidateType::ServerReflexive);

    // Exchange candidates via set_remote_candidates (simulates signaling).
    let a_local = agent_a.local_candidates().to_vec();
    let b_local = agent_b.local_candidates().to_vec();

    agent_a.set_remote_candidates(b_local);
    agent_b.set_remote_candidates(a_local);

    // Verify candidate pair formation on both sides.
    let a_pairs = agent_a.sorted_candidate_pairs();
    let b_pairs = agent_b.sorted_candidate_pairs();

    assert!(!a_pairs.is_empty());
    assert!(!b_pairs.is_empty());

    // Pairs should be sorted by descending combined priority.
    for pairs in &[&a_pairs, &b_pairs] {
        for window in pairs.windows(2) {
            let prio_0 = window[0].0.priority as u64 * 1_000_000 + window[0].1.priority as u64;
            let prio_1 = window[1].0.priority as u64 * 1_000_000 + window[1].1.priority as u64;
            assert!(
                prio_0 >= prio_1,
                "candidate pairs should be sorted by descending priority"
            );
        }
    }

    // Verify uniqueness of (local, remote) pairs.
    let a_pair_ids: HashSet<_> = a_pairs.iter()
        .map(|(l, r)| (l.id.clone(), r.id.clone()))
        .collect();
    assert_eq!(a_pair_ids.len(), a_pairs.len(), "all pairs should be unique");

    // No selected pair before connectivity checks.
    assert!(agent_a.selected_pair().is_none());

    // Reset clears everything.
    agent_a.reset();
    assert!(agent_a.local_candidates().is_empty());
    assert!(agent_a.selected_pair().is_none());
}

// ---------------------------------------------------------------------------
// Test 6: Flow-control backpressure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_flow_control_backpressure_integration() {
    let config = FlowConfig {
        send_buffer_capacity: 8,
        recv_buffer_capacity: 8,
        max_message_size: 1024 * 1024,
        message_ttl: Duration::from_secs(60),
    };
    let mut fc = FlowController::with_config(config);

    // Fill send buffer to capacity.
    for i in 0..8u8 {
        assert!(fc.try_send(Channel::Input, vec![i; 100]));
    }
    assert_eq!(fc.send_buffer_len(), 8);
    assert!(fc.is_send_backpressured());

    // 9th message must be rejected.
    assert!(!fc.try_send(Channel::Video, b"overflow".to_vec()));

    // Drain and verify integrity.
    let mut drained: Vec<(Channel, Vec<u8>)> = Vec::new();
    while let Some(item) = fc.poll_send() {
        drained.push(item);
    }
    assert_eq!(drained.len(), 8);
    assert_eq!(fc.send_buffer_len(), 0);
    assert!(!fc.is_send_backpressured());

    for (i, (ch, data)) in drained.iter().enumerate() {
        assert_eq!(*ch, Channel::Input);
        assert_eq!(data.len(), 100);
        assert_eq!(data[0], i as u8);
    }

    // Recv-buffer overflow: push more than capacity.
    for i in 0..10u8 {
        fc.try_recv(Channel::Video, vec![i; 200]);
    }
    // Capacity 8: items 0 and 1 dropped to make room for 8 and 9.
    assert_eq!(fc.recv_buffer_len(), 8);
    assert!(fc.is_recv_backpressured());

    let mut recv_items: Vec<u8> = Vec::new();
    while let Some((_, data)) = fc.poll_recv() {
        recv_items.push(data[0]);
    }
    assert_eq!(recv_items.len(), 8);
    assert!(!recv_items.contains(&0u8), "item 0 should have been dropped");
    assert!(!recv_items.contains(&1u8), "item 1 should have been dropped");
    for i in 2u8..=9 {
        assert!(recv_items.contains(&i), "item {} should be present", i);
    }
}

// ---------------------------------------------------------------------------
// Test 7: Reconnect flow (connect -> disconnect -> reconnect)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_reconnect_flow() {
    // Phase 1: initial connection.
    let server1 = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).unwrap();
    let addr1 = server1.local_addr().unwrap();
    let client1 = QuicEndpoint::new_client().unwrap();

    let mgr = ReconnectManager::new(client1, addr1);
    assert_eq!(mgr.state().await, ConnectionState::Disconnected);
    assert!(mgr.can_reconnect().await);

    let server1_handle = tokio::spawn(async move {
        server1.accept().await.unwrap()
    });

    let conn = mgr.connect().await.unwrap();
    let _st1 = server1_handle.await.unwrap();
    assert_eq!(mgr.state().await, ConnectionState::Connected);

    // Phase 2: disconnect.
    conn.close(0u32.into(), b"test");
    mgr.mark_disconnected().await;
    assert_eq!(mgr.state().await, ConnectionState::Disconnected);

    // Phase 3: reconnect to a fresh server.
    let server2 = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).unwrap();
    let addr2 = server2.local_addr().unwrap();
    let client2 = QuicEndpoint::new_client().unwrap();

    let mgr2 = ReconnectManager::new(client2, addr2);
    assert_eq!(mgr2.state().await, ConnectionState::Disconnected);

    let server2_handle = tokio::spawn(async move {
        let conn = server2.accept().await.unwrap();
        QuicTransport::new(conn, true)
    });

    let conn2 = mgr2.connect().await.unwrap();
    let mut st2 = server2_handle.await.unwrap();
    assert_eq!(mgr2.state().await, ConnectionState::Connected);

    // Verify data flows on the new connection.
    let mut transport2 = QuicTransport::new(conn2.clone(), true);
    let recv_task = tokio::spawn(async move {
        let ch = st2.accept_stream().await.unwrap();
        st2.recv(ch).await.unwrap()
    });

    use alldesk_net::Transport;
    transport2.send(Channel::Control, b"reconnected-ok").await.unwrap();
    let data = recv_task.await.unwrap();
    assert_eq!(&data, b"reconnected-ok");

    conn2.close(0u32.into(), b"test");
    mgr2.mark_disconnected().await;
    assert_eq!(mgr2.state().await, ConnectionState::Disconnected);

    // Phase 4: reconnect failure and reset.
    let client3 = QuicEndpoint::new_client().unwrap();
    let bad_addr: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
    let mgr3 = ReconnectManager::new(client3, bad_addr).with_max_attempts(2);
    assert!(mgr3.reconnect().await.is_err());
    assert_eq!(mgr3.state().await, ConnectionState::Failed);
    assert!(!mgr3.can_reconnect().await);

    mgr3.reset().await;
    assert_eq!(mgr3.state().await, ConnectionState::Disconnected);
    assert!(mgr3.can_reconnect().await);
}
