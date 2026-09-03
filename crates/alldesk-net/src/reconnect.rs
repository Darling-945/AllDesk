use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::quic_conn::QuicEndpoint;
use alldesk_core::error::Error;
use alldesk_core::Result;

/// Maximum number of reconnection attempts before giving up.
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

/// Initial delay before first reconnect attempt.
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);

/// Maximum backoff delay cap.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Multiplier for exponential backoff.
const BACKOFF_MULTIPLIER: f64 = 1.5;

/// Connection state tracked by the reconnect manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// No active connection.
    Disconnected,
    /// Currently establishing initial connection.
    Connecting,
    /// Connection is alive and working.
    Connected,
    /// Connection lost, attempting to reconnect.
    Reconnecting,
    /// All reconnect attempts exhausted.
    Failed,
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionState::Disconnected => write!(f, "disconnected"),
            ConnectionState::Connecting => write!(f, "connecting"),
            ConnectionState::Connected => write!(f, "connected"),
            ConnectionState::Reconnecting => write!(f, "reconnecting"),
            ConnectionState::Failed => write!(f, "failed"),
        }
    }
}

/// Manages automatic reconnection of QUIC connections with exponential backoff.
pub struct ReconnectManager {
    endpoint: QuicEndpoint,
    remote_addr: SocketAddr,
    state: Arc<Mutex<ConnectionState>>,
    max_attempts: u32,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl ReconnectManager {
    /// Create a new reconnect manager targeting the given address.
    pub fn new(endpoint: QuicEndpoint, remote_addr: SocketAddr) -> Self {
        Self {
            endpoint,
            remote_addr,
            state: Arc::new(Mutex::new(ConnectionState::Disconnected)),
            max_attempts: MAX_RECONNECT_ATTEMPTS,
            initial_backoff: INITIAL_BACKOFF,
            max_backoff: MAX_BACKOFF,
        }
    }

    /// Set custom max reconnect attempts.
    pub fn with_max_attempts(mut self, max: u32) -> Self {
        self.max_attempts = max;
        self
    }

    /// Get current connection state.
    pub async fn state(&self) -> ConnectionState {
        *self.state.lock().await
    }

    /// Perform initial connection to the remote peer.
    ///
    /// Returns the raw connection; the caller builds its own transports
    /// (several pipelines usually share one connection).
    pub async fn connect(&self) -> Result<quinn::Connection> {
        {
            let mut s = self.state.lock().await;
            if *s == ConnectionState::Connected {
                return Err(Error::Network("already connected".into()));
            }
            *s = ConnectionState::Connecting;
        }

        match self.endpoint.connect(self.remote_addr).await {
            Ok(conn) => {
                info!("Connected to {}", self.remote_addr);
                *self.state.lock().await = ConnectionState::Connected;
                Ok(conn)
            }
            Err(e) => {
                *self.state.lock().await = ConnectionState::Disconnected;
                Err(e)
            }
        }
    }

    /// Attempt to reconnect after a connection failure.
    /// Uses exponential backoff between attempts.
    /// Returns the new connection on success, or the last error on failure.
    pub async fn reconnect(&self) -> Result<quinn::Connection> {
        {
            let mut s = self.state.lock().await;
            *s = ConnectionState::Reconnecting;
        }

        let mut delay = self.initial_backoff;

        for attempt in 1..=self.max_attempts {
            debug!(
                "Reconnect attempt {}/{} to {}",
                attempt, self.max_attempts, self.remote_addr
            );

            tokio::time::sleep(delay).await;

            match self.endpoint.connect(self.remote_addr).await {
                Ok(conn) => {
                    info!("Reconnected to {} on attempt {}", self.remote_addr, attempt);
                    *self.state.lock().await = ConnectionState::Connected;
                    return Ok(conn);
                }
                Err(e) => {
                    warn!(
                        "Reconnect attempt {}/{} failed: {}",
                        attempt, self.max_attempts, e
                    );
                }
            }

            // Exponential backoff with jitter
            delay = std::cmp::min(
                Duration::from_secs_f64(delay.as_secs_f64() * BACKOFF_MULTIPLIER),
                self.max_backoff,
            );
        }

        *self.state.lock().await = ConnectionState::Failed;
        Err(Error::Network(format!(
            "failed to reconnect after {} attempts",
            self.max_attempts
        )))
    }

    /// Mark the connection as disconnected, triggering reconnect readiness.
    pub async fn mark_disconnected(&self) {
        let mut s = self.state.lock().await;
        if *s == ConnectionState::Connected {
            info!("Connection to {} marked as disconnected", self.remote_addr);
            *s = ConnectionState::Disconnected;
        }
    }

    /// Check if reconnection is possible (not in Failed state).
    pub async fn can_reconnect(&self) -> bool {
        let s = self.state.lock().await;
        !matches!(*s, ConnectionState::Failed)
    }

    /// Reset the manager to allow reconnecting after a Failed state.
    pub async fn reset(&self) {
        *self.state.lock().await = ConnectionState::Disconnected;
    }
}

/// Calculate backoff delay for a given attempt number (0-indexed).
pub fn backoff_delay(attempt: u32) -> Duration {
    let delay = INITIAL_BACKOFF.as_secs_f64() * BACKOFF_MULTIPLIER.powi(attempt as i32);
    std::cmp::min(Duration::from_secs_f64(delay), MAX_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_display() {
        assert_eq!(ConnectionState::Disconnected.to_string(), "disconnected");
        assert_eq!(ConnectionState::Connecting.to_string(), "connecting");
        assert_eq!(ConnectionState::Connected.to_string(), "connected");
        assert_eq!(ConnectionState::Reconnecting.to_string(), "reconnecting");
        assert_eq!(ConnectionState::Failed.to_string(), "failed");
    }

    #[test]
    fn test_backoff_delay_increases() {
        let d0 = backoff_delay(0);
        let d1 = backoff_delay(1);
        let d2 = backoff_delay(2);
        assert!(d1 > d0);
        assert!(d2 > d1);
    }

    #[test]
    fn test_backoff_delay_capped() {
        let d = backoff_delay(100);
        assert!(d <= MAX_BACKOFF);
    }

    #[tokio::test]
    async fn test_reconnect_manager_initial_state() {
        let endpoint = QuicEndpoint::new_client().unwrap();
        let addr: SocketAddr = "127.0.0.1:19999".parse().unwrap();
        let mgr = ReconnectManager::new(endpoint, addr);
        assert_eq!(mgr.state().await, ConnectionState::Disconnected);
        assert!(mgr.can_reconnect().await);
    }

    #[tokio::test]
    async fn test_reconnect_manager_connect_fail() {
        let endpoint = QuicEndpoint::new_client().unwrap();
        let addr: SocketAddr = "127.0.0.1:19998".parse().unwrap();
        let mgr = ReconnectManager::new(endpoint, addr);
        let result = mgr.connect().await;
        assert!(result.is_err());
        assert_eq!(mgr.state().await, ConnectionState::Disconnected);
    }

    #[tokio::test]
    async fn test_reconnect_manager_reconnect_fail_exhausted() {
        let endpoint = QuicEndpoint::new_client().unwrap();
        let addr: SocketAddr = "127.0.0.1:19997".parse().unwrap();
        let mgr = ReconnectManager::new(endpoint, addr).with_max_attempts(2);
        let result = mgr.reconnect().await;
        assert!(result.is_err());
        assert_eq!(mgr.state().await, ConnectionState::Failed);
        assert!(!mgr.can_reconnect().await);
    }

    #[tokio::test]
    async fn test_reconnect_manager_reset() {
        let endpoint = QuicEndpoint::new_client().unwrap();
        let addr: SocketAddr = "127.0.0.1:19996".parse().unwrap();
        let mgr = ReconnectManager::new(endpoint, addr).with_max_attempts(1);
        let _ = mgr.reconnect().await;
        assert_eq!(mgr.state().await, ConnectionState::Failed);
        mgr.reset().await;
        assert_eq!(mgr.state().await, ConnectionState::Disconnected);
        assert!(mgr.can_reconnect().await);
    }

    #[tokio::test]
    async fn test_reconnect_manager_mark_disconnected() {
        let endpoint = QuicEndpoint::new_client().unwrap();
        let addr: SocketAddr = "127.0.0.1:19995".parse().unwrap();
        let mgr = ReconnectManager::new(endpoint, addr);
        // Mark disconnected from Disconnected state should stay Disconnected
        mgr.mark_disconnected().await;
        assert_eq!(mgr.state().await, ConnectionState::Disconnected);
    }

    #[tokio::test]
    async fn test_reconnect_connect_then_reconnect() {
        let server = QuicEndpoint::new_server("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr().unwrap();

        let endpoint = QuicEndpoint::new_client().unwrap();
        let mgr = ReconnectManager::new(endpoint, addr);

        // Accept on server side
        let server_handle = tokio::spawn(async move { server.accept().await.unwrap() });

        let conn = mgr.connect().await.unwrap();
        let _server_conn = server_handle.await.unwrap();
        assert_eq!(mgr.state().await, ConnectionState::Connected);

        conn.close(0u32.into(), b"test");
        mgr.mark_disconnected().await;
        assert_eq!(mgr.state().await, ConnectionState::Disconnected);
    }
}
