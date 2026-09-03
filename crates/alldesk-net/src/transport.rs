use async_trait::async_trait;
use bytes::Bytes;
use futures::FutureExt;
use quinn::{Connection, RecvStream, SendStream, VarInt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::channel::Channel;
use alldesk_core::error::Error;
use alldesk_core::Result;

/// Maximum payload size for QUIC datagrams. 1200 bytes is safe for typical
/// MTU-limited paths; payloads larger than this fall back to reliable streams.
const MAX_DATAGRAM_SIZE: usize = 1200;

/// One accepted/opened bidirectional stream. Send and receive halves are
/// locked separately so two tasks can use the same channel in both
/// directions at once (e.g. bidirectional clipboard sync) without blocking
/// each other on a single stream-table lock held across I/O.
pub struct StreamPair {
    send: Arc<Mutex<SendStream>>,
    recv: Arc<Mutex<RecvStream>>,
}

/// Stream table shared by all transport handles wrapping the same connection.
pub type SharedStreams = Arc<Mutex<HashMap<u64, StreamPair>>>;

/// A transport over one QUIC connection. Cloning (or building via
/// `with_shared_streams`) yields another handle to the same stream table,
/// so per-pipeline transports can coexist on a single connection.
#[derive(Clone)]
pub struct QuicTransport {
    conn: Connection,
    is_p2p: bool,
    streams: SharedStreams,
}

impl QuicTransport {
    pub fn new(conn: Connection, is_p2p: bool) -> Self {
        Self {
            conn,
            is_p2p,
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a transport that shares `streams` (e.g. obtained from another
    /// transport's `shared_streams()`) so both handles see the same streams.
    pub fn with_shared_streams(conn: Connection, is_p2p: bool, streams: SharedStreams) -> Self {
        Self {
            conn,
            is_p2p,
            streams,
        }
    }

    /// The shared stream table of this transport.
    pub fn shared_streams(&self) -> SharedStreams {
        Arc::clone(&self.streams)
    }

    async fn ensure_stream(&self, channel: Channel) -> Result<()> {
        let id = channel.stream_id();
        {
            let streams = self.streams.lock().await;
            if streams.contains_key(&id) {
                return Ok(());
            }
        }
        let (mut send, recv) = self
            .conn
            .open_bi()
            .await
            .map_err(|e| Error::Network(format!("open stream: {}", e)))?;
        // Send channel ID as first byte so the peer can route the stream.
        send.write_all(&[id as u8])
            .await
            .map_err(|e| Error::Network(format!("write channel id: {}", e)))?;
        let mut streams = self.streams.lock().await;
        streams.entry(id).or_insert_with(|| StreamPair {
            send: Arc::new(Mutex::new(send)),
            recv: Arc::new(Mutex::new(recv)),
        });
        Ok(())
    }

    /// Accept a single incoming bidirectional stream, read its channel ID
    /// prefix, and register it in the stream map. Returns the channel.
    pub async fn accept_stream(&self) -> Result<Channel> {
        let (send, mut recv) = self
            .conn
            .accept_bi()
            .await
            .map_err(|e| Error::Network(format!("accept stream: {}", e)))?;
        let mut id_buf = [0u8; 1];
        recv.read_exact(&mut id_buf)
            .await
            .map_err(|e| Error::Network(format!("read channel id: {}", e)))?;
        let id = id_buf[0] as u64;
        let channel = Channel::from_stream_id(id)
            .ok_or_else(|| Error::Network(format!("unknown channel id: {}", id)))?;
        let mut streams = self.streams.lock().await;
        streams.entry(id).or_insert_with(|| StreamPair {
            send: Arc::new(Mutex::new(send)),
            recv: Arc::new(Mutex::new(recv)),
        });
        Ok(channel)
    }

    /// Wait until the stream for `wanted` is available, accepting and
    /// registering incoming streams for other channels along the way.
    ///
    /// Unlike `accept_stream`, streams opened for sibling pipelines are not
    /// rejected: they land in the shared stream map, so multiple pipelines
    /// sharing one connection can each wait for their own channel without
    /// stealing each other's streams.
    pub async fn accept_channel(&self, wanted: Channel) -> Result<()> {
        // Poll the accept future instead of awaiting it, so the shared map
        // can be re-checked for channels registered by a sibling pipeline.
        let mut accept = Box::pin(self.conn.accept_bi());
        loop {
            if self.streams.lock().await.contains_key(&wanted.stream_id()) {
                return Ok(());
            }
            if let Some(result) = accept.as_mut().now_or_never() {
                let (send, mut recv) =
                    result.map_err(|e| Error::Network(format!("accept stream: {}", e)))?;
                let mut id_buf = [0u8; 1];
                recv.read_exact(&mut id_buf)
                    .await
                    .map_err(|e| Error::Network(format!("read channel id: {}", e)))?;
                let id = id_buf[0] as u64;
                Channel::from_stream_id(id)
                    .ok_or_else(|| Error::Network(format!("unknown channel id: {}", id)))?;
                let mut streams = self.streams.lock().await;
                streams.entry(id).or_insert_with(|| StreamPair {
                    send: Arc::new(Mutex::new(send)),
                    recv: Arc::new(Mutex::new(recv)),
                });
                accept = Box::pin(self.conn.accept_bi());
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
    }

    /// Convenience helper: accept incoming streams for all the given channels.
    pub async fn accept_streams(&self, channels: &[Channel]) -> Result<()> {
        for _ in channels {
            self.accept_stream().await?;
        }
        Ok(())
    }

    /// Read one length-prefixed message from the stream registered for
    /// `channel`. Only the receive half of the stream is locked.
    async fn recv_on_stream(&self, channel: Channel) -> Result<Vec<u8>> {
        let recv_half = {
            let streams = self.streams.lock().await;
            match streams.get(&channel.stream_id()) {
                Some(pair) => Arc::clone(&pair.recv),
                None => return Err(Error::Network("stream not open".into())),
            }
        };
        let mut recv = recv_half.lock().await;
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| Error::Network(format!("read len: {}", e)))?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf)
            .await
            .map_err(|e| Error::Network(format!("read data: {}", e)))?;
        Ok(buf)
    }
}

#[async_trait]
impl crate::Transport for QuicTransport {
    async fn send(&mut self, channel: Channel, data: &[u8]) -> Result<()> {
        if channel.is_datagram() && data.len() <= MAX_DATAGRAM_SIZE {
            self.conn
                .send_datagram(Bytes::copy_from_slice(data))
                .map_err(|e| Error::Network(format!("send datagram: {}", e)))?;
        } else {
            // Stream channels (and datagrams too large for the MTU) go over
            // a reliable, length-prefixed stream.
            self.ensure_stream(channel).await?;
            let send_half = {
                let streams = self.streams.lock().await;
                match streams.get(&channel.stream_id()) {
                    Some(pair) => Arc::clone(&pair.send),
                    None => return Err(Error::Network("stream not open".into())),
                }
            };
            let mut send = send_half.lock().await;
            let len = (data.len() as u32).to_le_bytes();
            send.write_all(&len)
                .await
                .map_err(|e| Error::Network(format!("write len: {}", e)))?;
            send.write_all(data)
                .await
                .map_err(|e| Error::Network(format!("write data: {}", e)))?;
        }
        Ok(())
    }

    async fn recv(&mut self, channel: Channel) -> Result<Vec<u8>> {
        if channel.is_datagram() {
            // Datagrams are the preferred path; a large payload may have
            // fallen back to the reliable stream, so drain datagrams first
            // and only then read from the stream.
            let mut pending = Box::pin(self.conn.read_datagram());
            if let Some(Ok(dg)) = pending.as_mut().now_or_never() {
                return Ok(dg.to_vec());
            }
            let has_stream = self.streams.lock().await.contains_key(&channel.stream_id());
            if has_stream {
                return self.recv_on_stream(channel).await;
            }
            let data = pending
                .await
                .map_err(|e| Error::Network(format!("read datagram: {}", e)))?;
            Ok(data.to_vec())
        } else {
            self.recv_on_stream(channel).await
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.conn.close(VarInt::from_u32(0), b"done");
        Ok(())
    }

    fn is_p2p(&self) -> bool {
        self.is_p2p
    }

    fn rtt_ms(&self) -> f64 {
        self.conn.rtt().as_secs_f64() * 1000.0
    }
}
