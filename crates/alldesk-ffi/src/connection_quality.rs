//! Connection quality metrics for the AllDesk FFI layer.
//!
//! Provides [`ConnectionQuality`] and [`QualityCollector`] so the Flutter UI
//! can display latency, packet loss, bandwidth and an overall quality level.

use std::time::Instant;

/// Maximum number of RTT samples retained for median computation.
const RTT_WINDOW: usize = 20;

// ---------------------------------------------------------------------------
// QualityLevel
// ---------------------------------------------------------------------------

/// Overall connection quality classification.
#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
pub enum QualityLevel {
    /// RTT < 20 ms, packet loss < 1 %
    Excellent,
    /// RTT < 50 ms, packet loss < 3 %
    Good,
    /// RTT < 100 ms, packet loss < 5 %
    Fair,
    /// RTT < 200 ms, packet loss < 10 %
    Poor,
    /// RTT >= 200 ms or packet loss >= 10 %
    Bad,
}

// ---------------------------------------------------------------------------
// ConnectionQuality
// ---------------------------------------------------------------------------

/// Snapshot of connection quality metrics, ready to be sent to Flutter.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectionQuality {
    /// Round-trip time in milliseconds.
    pub rtt_ms: f64,
    /// Packet loss ratio (0.0 – 1.0).
    pub packet_loss: f64,
    /// Estimated bandwidth in kbps.
    pub bandwidth_kbps: u64,
    /// Connection quality level.
    pub quality: QualityLevel,
    /// Timestamp of last measurement, as milliseconds since the collector was created.
    pub last_updated_ms: u64,
    /// Number of frames received.
    pub frames_received: u64,
    /// Number of frames dropped.
    pub frames_dropped: u64,
}

// ---------------------------------------------------------------------------
// QualityCollector
// ---------------------------------------------------------------------------

/// Accumulates transport metrics and produces [`ConnectionQuality`] snapshots.
pub struct QualityCollector {
    rtt_samples: Vec<f64>,
    total_sent: u64,
    total_lost: u64,
    frames_received: u64,
    frames_dropped: u64,
    bandwidth_kbps: u64,
    last_update: Instant,
    created: Instant,
}

impl QualityCollector {
    /// Create an empty collector.
    pub fn new() -> Self {
        Self {
            rtt_samples: Vec::with_capacity(RTT_WINDOW),
            total_sent: 0,
            total_lost: 0,
            frames_received: 0,
            frames_dropped: 0,
            bandwidth_kbps: 0,
            last_update: Instant::now(),
            created: Instant::now(),
        }
    }

    /// Record a round-trip time sample (milliseconds).
    pub fn record_rtt(&mut self, ms: f64) {
        if self.rtt_samples.len() >= RTT_WINDOW {
            // Slide the window: drop the oldest sample.
            self.rtt_samples.remove(0);
        }
        self.rtt_samples.push(ms);
    }

    /// Increment the count of packets sent.
    pub fn record_packet_sent(&mut self) {
        self.total_sent += 1;
    }

    /// Increment the count of packets lost.
    pub fn record_packet_lost(&mut self) {
        self.total_lost += 1;
    }

    /// Increment the count of frames received.
    pub fn record_frame_received(&mut self) {
        self.frames_received += 1;
    }

    /// Increment the count of frames dropped.
    pub fn record_frame_dropped(&mut self) {
        self.frames_dropped += 1;
    }

    /// Update the estimated bandwidth (kbps).
    pub fn set_bandwidth(&mut self, kbps: u64) {
        self.bandwidth_kbps = kbps;
    }

    /// Compute a [`ConnectionQuality`] snapshot from the collected samples.
    ///
    /// Uses the median of the last `RTT_WINDOW` RTT samples and the cumulative
    /// packet-loss ratio.  The RTT sample window is reset after each call.
    pub fn compute_quality(&mut self) -> ConnectionQuality {
        let rtt_ms = median(&self.rtt_samples);
        let packet_loss = if self.total_sent > 0 {
            self.total_lost as f64 / self.total_sent as f64
        } else {
            0.0
        };
        let quality = classify(rtt_ms, packet_loss);

        let now = Instant::now();
        let last_updated_ms = now.duration_since(self.created).as_millis() as u64;
        self.last_update = now;

        let result = ConnectionQuality {
            rtt_ms,
            packet_loss,
            bandwidth_kbps: self.bandwidth_kbps,
            quality,
            last_updated_ms,
            frames_received: self.frames_received,
            frames_dropped: self.frames_dropped,
        };

        // Reset the RTT sample window for the next measurement period.
        self.rtt_samples.clear();

        result
    }

    /// Clear all collected samples and counters.
    pub fn reset(&mut self) {
        self.rtt_samples.clear();
        self.total_sent = 0;
        self.total_lost = 0;
        self.frames_received = 0;
        self.frames_dropped = 0;
        self.bandwidth_kbps = 0;
        self.last_update = Instant::now();
        self.created = Instant::now();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the median of a slice of `f64` values.
/// Returns 0.0 for an empty slice.
fn median(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

/// Classify connection quality based on RTT and packet loss thresholds.
fn classify(rtt_ms: f64, packet_loss: f64) -> QualityLevel {
    // Check "worst" first so that bad latency cannot be hidden by good loss
    // and vice-versa.
    if rtt_ms >= 200.0 || packet_loss >= 0.10 {
        QualityLevel::Bad
    } else if rtt_ms < 20.0 && packet_loss < 0.01 {
        QualityLevel::Excellent
    } else if rtt_ms < 50.0 && packet_loss < 0.03 {
        QualityLevel::Good
    } else if rtt_ms < 100.0 && packet_loss < 0.05 {
        QualityLevel::Fair
    } else {
        QualityLevel::Poor
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_collector_new() {
        let c = QualityCollector::new();
        assert!(c.rtt_samples.is_empty());
        assert_eq!(c.total_sent, 0);
        assert_eq!(c.total_lost, 0);
        assert_eq!(c.frames_received, 0);
        assert_eq!(c.frames_dropped, 0);
        assert_eq!(c.bandwidth_kbps, 0);
    }

    #[test]
    fn test_quality_level_excellent() {
        assert_eq!(classify(10.0, 0.005), QualityLevel::Excellent);
        assert_eq!(classify(19.9, 0.009), QualityLevel::Excellent);
    }

    #[test]
    fn test_quality_level_poor() {
        // Poor: RTT 100-200 ms or loss 5-10 % (but neither reaching Bad)
        assert_eq!(classify(150.0, 0.01), QualityLevel::Poor);
        assert_eq!(classify(50.0, 0.07), QualityLevel::Poor);
        assert_eq!(classify(120.0, 0.08), QualityLevel::Poor);
    }

    #[test]
    fn test_quality_level_bad() {
        assert_eq!(classify(250.0, 0.0), QualityLevel::Bad);
        assert_eq!(classify(10.0, 0.15), QualityLevel::Bad);
        assert_eq!(classify(200.0, 0.0), QualityLevel::Bad);
        assert_eq!(classify(10.0, 0.10), QualityLevel::Bad);
    }

    #[test]
    fn test_quality_level_good() {
        assert_eq!(classify(30.0, 0.02), QualityLevel::Good);
        assert_eq!(classify(49.9, 0.029), QualityLevel::Good);
    }

    #[test]
    fn test_quality_level_fair() {
        assert_eq!(classify(70.0, 0.04), QualityLevel::Fair);
        assert_eq!(classify(99.9, 0.049), QualityLevel::Fair);
    }

    #[test]
    fn test_quality_collector_rtt() {
        let mut c = QualityCollector::new();
        c.record_rtt(10.0);
        c.record_rtt(20.0);
        c.record_rtt(30.0);
        assert_eq!(c.rtt_samples.len(), 3);

        let q = c.compute_quality();
        // Median of [10, 20, 30] = 20.0
        assert!((q.rtt_ms - 20.0).abs() < f64::EPSILON);
        // RTT sample window should be cleared after compute.
        assert!(c.rtt_samples.is_empty());
    }

    #[test]
    fn test_quality_collector_packet_loss() {
        let mut c = QualityCollector::new();
        for _ in 0..100 {
            c.record_packet_sent();
        }
        for _ in 0..5 {
            c.record_packet_lost();
        }
        let q = c.compute_quality();
        assert!((q.packet_loss - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn test_quality_collector_frames() {
        let mut c = QualityCollector::new();
        c.record_frame_received();
        c.record_frame_received();
        c.record_frame_received();
        c.record_frame_dropped();

        let q = c.compute_quality();
        assert_eq!(q.frames_received, 3);
        assert_eq!(q.frames_dropped, 1);
    }

    #[test]
    fn test_quality_collector_reset() {
        let mut c = QualityCollector::new();
        c.record_rtt(50.0);
        c.record_packet_sent();
        c.record_packet_lost();
        c.record_frame_received();
        c.record_frame_dropped();
        c.set_bandwidth(5000);

        c.reset();

        assert!(c.rtt_samples.is_empty());
        assert_eq!(c.total_sent, 0);
        assert_eq!(c.total_lost, 0);
        assert_eq!(c.frames_received, 0);
        assert_eq!(c.frames_dropped, 0);
        assert_eq!(c.bandwidth_kbps, 0);
    }

    #[test]
    fn test_rtt_window_sliding() {
        let mut c = QualityCollector::new();
        // Fill beyond the window size.
        for i in 0..(RTT_WINDOW + 5) {
            c.record_rtt(i as f64);
        }
        // Should keep at most RTT_WINDOW samples.
        assert_eq!(c.rtt_samples.len(), RTT_WINDOW);
        // The oldest samples should have been dropped; first remaining = 5.
        assert!((c.rtt_samples[0] - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_median_empty() {
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn test_median_even_count() {
        // [1, 2] -> median = 1.5
        assert!((median(&[1.0, 2.0]) - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_median_odd_count() {
        // [3, 1, 2] -> sorted [1, 2, 3] -> median = 2
        assert!((median(&[3.0, 1.0, 2.0]) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compute_quality_last_updated_advances() {
        let mut c = QualityCollector::new();
        let q1 = c.compute_quality();
        // Tiny sleep so the timestamp actually advances.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let q2 = c.compute_quality();
        assert!(q2.last_updated_ms >= q1.last_updated_ms);
    }
}
