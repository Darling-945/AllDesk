//! Flow control and backpressure for transport layers.
//!
//! Provides a wrapper around `Transport` that limits sending rate and
//! buffers received data to prevent a fast sender from overwhelming
//! a slow receiver.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::channel::Channel;

/// Maximum number of messages allowed in the send buffer before backpressure kicks in.
const DEFAULT_SEND_BUFFER_CAPACITY: usize = 64;

/// Maximum number of messages in the recv buffer.
const DEFAULT_RECV_BUFFER_CAPACITY: usize = 128;

/// Maximum size of a single message (1 MB).
const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// A buffered message waiting to be processed.
#[derive(Debug)]
struct BufferedMessage {
    channel: Channel,
    data: Vec<u8>,
    queued_at: Instant,
}

/// Per-channel send statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct ChannelStats {
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_recv: u64,
    /// Number of messages sent.
    pub msgs_sent: u64,
    /// Number of messages received.
    pub msgs_recv: u64,
    /// Number of messages dropped due to buffer overflow.
    pub msgs_dropped: u64,
}

/// Flow control configuration.
#[derive(Debug, Clone)]
pub struct FlowConfig {
    /// Maximum send buffer capacity in messages.
    pub send_buffer_capacity: usize,
    /// Maximum receive buffer capacity in messages.
    pub recv_buffer_capacity: usize,
    /// Maximum message size in bytes.
    pub max_message_size: usize,
    /// Time-to-live for buffered messages. Messages older than this are dropped.
    pub message_ttl: Duration,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            send_buffer_capacity: DEFAULT_SEND_BUFFER_CAPACITY,
            recv_buffer_capacity: DEFAULT_RECV_BUFFER_CAPACITY,
            max_message_size: MAX_MESSAGE_SIZE,
            message_ttl: Duration::from_secs(30),
        }
    }
}

/// Flow controller that applies backpressure when buffers are full.
pub struct FlowController {
    config: FlowConfig,
    /// Send buffer (outgoing messages waiting to be sent).
    send_buffer: VecDeque<BufferedMessage>,
    /// Receive buffer (incoming messages waiting to be consumed).
    recv_buffer: VecDeque<BufferedMessage>,
    /// Per-channel statistics.
    stats: Arc<Mutex<Vec<ChannelStats>>>,
}

impl FlowController {
    /// Create a new flow controller with default configuration.
    pub fn new() -> Self {
        Self::with_config(FlowConfig::default())
    }

    /// Create a new flow controller with custom configuration.
    pub fn with_config(config: FlowConfig) -> Self {
        let num_channels = 7; // Video, Audio, Input, Clipboard, File, Whiteboard, Control
        let mut stats = Vec::with_capacity(num_channels);
        for _ in 0..num_channels {
            stats.push(ChannelStats::default());
        }
        Self {
            config,
            send_buffer: VecDeque::new(),
            recv_buffer: VecDeque::new(),
            stats: Arc::new(Mutex::new(stats)),
        }
    }

    /// Try to enqueue a message for sending. Returns false if the buffer is full (backpressure).
    pub fn try_send(&mut self, channel: Channel, data: Vec<u8>) -> bool {
        if data.len() > self.config.max_message_size {
            return false;
        }

        // Drop expired messages from the send buffer
        drop_expired(&mut self.send_buffer, self.config.message_ttl);

        if self.send_buffer.len() >= self.config.send_buffer_capacity {
            return false;
        }

        self.send_buffer.push_back(BufferedMessage {
            channel,
            data,
            queued_at: Instant::now(),
        });
        true
    }

    /// Try to enqueue a received message. Returns false if buffer is full.
    pub fn try_recv(&mut self, channel: Channel, data: Vec<u8>) -> bool {
        if data.len() > self.config.max_message_size {
            return false;
        }

        // Drop expired messages from recv buffer
        drop_expired(&mut self.recv_buffer, self.config.message_ttl);

        if self.recv_buffer.len() >= self.config.recv_buffer_capacity {
            // Drop oldest message to make room
            self.recv_buffer.pop_front();
        }

        self.recv_buffer.push_back(BufferedMessage {
            channel,
            data,
            queued_at: Instant::now(),
        });
        true
    }

    /// Dequeue the next message to send.
    pub fn poll_send(&mut self) -> Option<(Channel, Vec<u8>)> {
        self.send_buffer.pop_front().map(|m| (m.channel, m.data))
    }

    /// Dequeue the next received message.
    pub fn poll_recv(&mut self) -> Option<(Channel, Vec<u8>)> {
        self.recv_buffer.pop_front().map(|m| (m.channel, m.data))
    }

    /// Check if the send buffer has backpressure (is near capacity).
    pub fn is_send_backpressured(&self) -> bool {
        self.send_buffer.len() >= self.config.send_buffer_capacity * 3 / 4
    }

    /// Check if the recv buffer has backpressure.
    pub fn is_recv_backpressured(&self) -> bool {
        self.recv_buffer.len() >= self.config.recv_buffer_capacity * 3 / 4
    }

    /// Number of messages in the send buffer.
    pub fn send_buffer_len(&self) -> usize {
        self.send_buffer.len()
    }

    /// Number of messages in the recv buffer.
    pub fn recv_buffer_len(&self) -> usize {
        self.recv_buffer.len()
    }

    /// Get per-channel statistics.
    pub async fn stats(&self) -> Vec<ChannelStats> {
        self.stats.lock().await.clone()
    }

    /// Record that a message was sent on a channel.
    pub async fn record_sent(&self, channel: Channel, bytes: usize) {
        let mut stats = self.stats.lock().await;
        let idx = channel.stream_id() as usize;
        if idx < stats.len() {
            stats[idx].bytes_sent += bytes as u64;
            stats[idx].msgs_sent += 1;
        }
    }

    /// Record that a message was received on a channel.
    pub async fn record_recv(&self, channel: Channel, bytes: usize) {
        let mut stats = self.stats.lock().await;
        let idx = channel.stream_id() as usize;
        if idx < stats.len() {
            stats[idx].bytes_recv += bytes as u64;
            stats[idx].msgs_recv += 1;
        }
    }

}

/// Drop expired messages from a buffer.
fn drop_expired(buffer: &mut VecDeque<BufferedMessage>, ttl: Duration) {
    let now = Instant::now();
    while let Some(front) = buffer.front() {
        if now.duration_since(front.queued_at) > ttl {
            buffer.pop_front();
        } else {
            break;
        }
    }
}

impl Default for FlowController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_controller_new() {
        let fc = FlowController::new();
        assert_eq!(fc.send_buffer_len(), 0);
        assert_eq!(fc.recv_buffer_len(), 0);
        assert!(!fc.is_send_backpressured());
        assert!(!fc.is_recv_backpressured());
    }

    #[test]
    fn test_flow_controller_send_recv() {
        let mut fc = FlowController::new();
        assert!(fc.try_send(Channel::Input, b"hello".to_vec()));
        assert_eq!(fc.send_buffer_len(), 1);

        let (ch, data) = fc.poll_send().unwrap();
        assert_eq!(ch, Channel::Input);
        assert_eq!(&data, b"hello");
        assert_eq!(fc.send_buffer_len(), 0);
    }

    #[test]
    fn test_flow_controller_backpressure() {
        let config = FlowConfig {
            send_buffer_capacity: 4,
            ..FlowConfig::default()
        };
        let mut fc = FlowController::with_config(config);

        // Fill the send buffer
        for i in 0..4 {
            assert!(fc.try_send(Channel::Input, vec![i as u8]));
        }
        assert!(fc.is_send_backpressured());

        // Should reject when full
        assert!(!fc.try_send(Channel::Input, b"overflow".to_vec()));
    }

    #[test]
    fn test_flow_controller_max_message_size() {
        let mut fc = FlowController::new();
        let big = vec![0u8; MAX_MESSAGE_SIZE + 1];
        assert!(!fc.try_send(Channel::Input, big));
    }

    #[test]
    fn test_flow_controller_recv_overflow_drops_oldest() {
        let config = FlowConfig {
            recv_buffer_capacity: 2,
            ..FlowConfig::default()
        };
        let mut fc = FlowController::with_config(config);

        fc.try_recv(Channel::Input, b"first".to_vec());
        fc.try_recv(Channel::Input, b"second".to_vec());
        // Third should drop the oldest
        fc.try_recv(Channel::Input, b"third".to_vec());

        let (_, data) = fc.poll_recv().unwrap();
        assert_eq!(&data, b"second"); // "first" was dropped
    }

    #[tokio::test]
    async fn test_flow_controller_stats() {
        let fc = FlowController::new();
        fc.record_sent(Channel::Input, 100).await;
        fc.record_recv(Channel::Input, 200).await;

        let stats = fc.stats().await;
        let input_stats = &stats[Channel::Input.stream_id() as usize];
        assert_eq!(input_stats.bytes_sent, 100);
        assert_eq!(input_stats.bytes_recv, 200);
        assert_eq!(input_stats.msgs_sent, 1);
        assert_eq!(input_stats.msgs_recv, 1);
    }

    #[test]
    fn test_flow_config_default() {
        let config = FlowConfig::default();
        assert_eq!(config.send_buffer_capacity, DEFAULT_SEND_BUFFER_CAPACITY);
        assert_eq!(config.recv_buffer_capacity, DEFAULT_RECV_BUFFER_CAPACITY);
        assert_eq!(config.max_message_size, MAX_MESSAGE_SIZE);
    }
}
