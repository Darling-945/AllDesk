use metrics::{counter, gauge};
use metrics_exporter_prometheus::PrometheusBuilder;
use tracing::info;

/// Default port for the Prometheus metrics HTTP endpoint.
const DEFAULT_METRICS_PORT: u16 = 21121;

/// Initialize the Prometheus metrics exporter.
///
/// Starts an HTTP server on the given port that serves `/metrics` for Prometheus
/// scraping. Also installs a global metrics recorder.
pub fn init_metrics(port: u16) -> anyhow::Result<()> {
    let port = if port == 0 {
        DEFAULT_METRICS_PORT
    } else {
        port
    };

    PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], port))
        .install()?;

    info!("Prometheus metrics exporter listening on port {}", port);
    Ok(())
}

/// Record a new WebSocket connection event.
pub fn record_ws_connection() {
    counter!("alldesk_ws_connections_total").increment(1);
}

/// Record a WebSocket disconnection event.
pub fn record_ws_disconnection() {
    counter!("alldesk_ws_disconnections_total").increment(1);
}

/// Record a signaling message by type.
pub fn record_signaling_message(msg_type: &str) {
    counter!("alldesk_signaling_messages_total", "type" => msg_type.to_string()).increment(1);
}

/// Record a STUN binding request.
pub fn record_stun_request() {
    counter!("alldesk_stun_requests_total").increment(1);
}

/// Record a new relay session creation.
pub fn record_relay_session() {
    counter!("alldesk_relay_sessions_total").increment(1);
}

/// Update the gauge for the current number of active registered peers.
pub fn record_active_peers(count: usize) {
    gauge!("alldesk_active_peers").set(count as f64);
}

/// Update the gauge for the current number of active relay sessions.
pub fn record_active_relay_sessions(count: usize) {
    gauge!("alldesk_active_relay_sessions").set(count as f64);
}

/// Record a new TURN relay allocation.
pub fn record_turn_allocation() {
    counter!("alldesk_turn_allocations_total").increment(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    static RECORDER: OnceLock<metrics_util::debugging::Snapshotter> = OnceLock::new();

    /// Get (or lazily install) the shared test recorder and return its snapshotter.
    fn get_snapshotter() -> &'static metrics_util::debugging::Snapshotter {
        RECORDER.get_or_init(|| {
            let recorder = metrics_util::debugging::DebuggingRecorder::new();
            let snapshotter = recorder.snapshotter();
            let _ = recorder.install();
            snapshotter
        })
    }

    #[test]
    fn test_record_ws_connection_increments_counter() {
        let snapshotter = get_snapshotter();

        record_ws_connection();

        let snap = snapshotter.snapshot();
        let entries = snap.into_vec();
        let ws_entry = entries
            .iter()
            .find(|(k, _, _, _)| k.key().name() == "alldesk_ws_connections_total");

        assert!(
            ws_entry.is_some(),
            "ws_connections counter should have been recorded"
        );

        if let Some((_, _, _, metrics_util::debugging::DebugValue::Counter(val))) = ws_entry {
            assert!(
                *val >= 1,
                "counter should have been incremented at least once, got {}",
                val
            );
        }
    }

    #[test]
    fn test_record_stun_request_increments_counter() {
        let snapshotter = get_snapshotter();

        record_stun_request();

        let snap = snapshotter.snapshot();
        let found = snap
            .into_vec()
            .iter()
            .any(|(k, _, _, _)| k.key().name() == "alldesk_stun_requests_total");
        assert!(found, "STUN request counter should have been recorded");
    }

    #[test]
    fn test_record_active_peers_sets_gauge() {
        let snapshotter = get_snapshotter();

        record_active_peers(42);

        let snap = snapshotter.snapshot();
        let entries = snap.into_vec();
        let gauge_entry = entries
            .into_iter()
            .find(|(k, _, _, _)| k.key().name() == "alldesk_active_peers");

        assert!(
            gauge_entry.is_some(),
            "active peers gauge should have been recorded"
        );

        if let Some((_, _, _, metrics_util::debugging::DebugValue::Gauge(val))) = gauge_entry {
            assert_eq!(val.0, 42.0, "gauge should be set to 42");
        }
    }
}
