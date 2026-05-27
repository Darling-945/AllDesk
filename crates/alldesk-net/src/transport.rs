use async_trait::async_trait;
use bytes::Bytes;
use quinn::{Connection, SendStream, RecvStream, VarInt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use alldesk_core::Result;
use alldesk_core::error::Error;
use crate::channel::Channel;

/// Maximum payload size for QUIC datagrams. 1200 bytes is safe for typical
/// MTU-limited paths; payloads larger than this fall back to reliable streams.
const MAX_DATAGRAM_SIZE: usize = 1200;

struct StreamPair {
    send: SendStream,
    recv: RecvStream,
}

pub struct QuicTransport {
    conn: Connection,
    is_p2p: bool,
    streams: Arc<Mutex<HashMap<u64, StreamPair>>>,
}

impl QuicTransport {
    pub fn new(conn: Connection, is_p2p: bool) -> Self {
        Self {
            conn,
            is_p2p,
            streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn ensure_stream(&self, channel: Channel) -> Result<()> {
        let id = channel.stream_id();
        let mut streams = self.streams.lock().await;
        if streams.contains_key(&id) {
            return Ok(());
        }
        let (mut send, recv) = self.conn.open_bi().await
            .map_err(|e| Error::Network(format!("open stream: {}", e)))?;
        // Send channel ID as first byte so the peer can route the stream.
        send.write_all(&[id as u8]).await
            .map_err(|e| Error::Network(format!("write channel id: {}", e)))?;
        streams.insert(id, StreamPair { send, recv });
        Ok(())
    }

    /// Accept a single incoming bidirectional stream, read its channel ID
    /// prefix, and register it in the stream map. Returns the channel.
    pub async fn accept_stream(&self) -> Result<Channel> {
        let (send, mut recv) = self.conn.accept_bi().await
            .map_err(|e| Error::Network(format!("accept stream: {}", e)))?;
        let mut id_buf = [0u8; 1];
        recv.read_exact(&mut id_buf).await
            .map_err(|e| Error::Network(format!("read channel id: {}", e)))?;
        let id = id_buf[0] as u64;
        let channel = Channel::from_stream_id(id)
            .ok_or_else(|| Error::Network(format!("unknown channel id: {}", id)))?;
        let mut streams = self.streams.lock().await;
        streams.insert(id, StreamPair { send, recv });
        Ok(channel)
    }

    /// Convenience helper: accept incoming streams for all the given channels.
    pub async fn accept_streams(&self, channels: &[Channel]) -> Result<()> {
        for _ in channels {
            self.accept_stream().await?;
        }
        Ok(())
    }
}

#[async_trait]
impl crate::Transport for QuicTransport {
    async fn send(&mut self, channel: Channel, data: &[u8]) -> Result<()> {
        if channel.is_datagram() {
            if data.len() <= MAX_DATAGRAM_SIZE {
                self.conn.send_datagram(Bytes::copy_from_slice(data))
                    .map_err(|e| Error::Network(format!("send datagram: {}", e)))?;
            } else {
                // Datagram too large — fall back to reliable stream transport.
                self.ensure_stream(channel).await?;
                let mut streams = self.streams.lock().await;
                let pair = streams.get_mut(&channel.stream_id())
                    .ok_or_else(|| Error::Network("stream not open".into()))?;
                let len = (data.len() as u32).to_le_bytes();
                pair.send.write_all(&len).await
                    .map_err(|e| Error::Network(format!("write len: {}", e)))?;
                pair.send.write_all(data).await
                    .map_err(|e| Error::Network(format!("write data: {}", e)))?;
            }
        } else {
            self.ensure_stream(channel).await?;
            let mut streams = self.streams.lock().await;
            let pair = streams.get_mut(&channel.stream_id())
                .ok_or_else(|| Error::Network("stream not open".into()))?;
            let len = (data.len() as u32).to_le_bytes();
            pair.send.write_all(&len).await
                .map_err(|e| Error::Network(format!("write len: {}", e)))?;
            pair.send.write_all(data).await
                .map_err(|e| Error::Network(format!("write data: {}", e)))?;
        }
        Ok(())
    }

    async fn recv(&mut self, channel: Channel) -> Result<Vec<u8>> {
        if channel.is_datagram() {
            let data = self.conn.read_datagram().await
                .map_err(|e| Error::Network(format!("read datagram: {}", e)))?;
            Ok(data.to_vec())
        } else {
            let mut streams = self.streams.lock().await;
            let pair = streams.get_mut(&channel.stream_id())
                .ok_or_else(|| Error::Network("stream not open".into()))?;
            let mut len_buf = [0u8; 4];
            pair.recv.read_exact(&mut len_buf).await
                .map_err(|e| Error::Network(format!("read len: {}", e)))?;
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            pair.recv.read_exact(&mut buf).await
                .map_err(|e| Error::Network(format!("read data: {}", e)))?;
            Ok(buf)
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
