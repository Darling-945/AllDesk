//! Receiver-side bandwidth estimation for adaptive streaming.
//!
//! Implements a simple delay-based bandwidth estimation (BWE) algorithm
//! inspired by GCC (Google Congestion Control). Monitors inter-arrival
//! time deltas to detect congestion and estimate available bandwidth.

use std::time::Instant;

/// Default initial bandwidth estimate in kbps.
const INITIAL_BANDWIDTH_KBPS: u32 = 2000;

/// Minimum bandwidth estimate in kbps.
const MIN_BANDWIDTH_KBPS: u32 = 100;

/// Maximum bandwidth estimate in kbps.
const MAX_BANDWIDTH_KBPS: u32 = 20000;

/// Number of recent samples to use for delay trend estimation.
const TREND_WINDOW_SIZE: usize = 20;

/// Threshold for delay trend to trigger decrease (0.0 - 1.0).
const OVERUSE_THRESHOLD: f64 = 0.6;

/// Threshold for delay trend to trigger increase.
const UNDERUSE_THRESHOLD: f64 = 0.2;

/// Bandwidth estimation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BweState {
    /// Bandwidth is under-utilized, can increase.
    Underuse,
    /// Bandwidth is stable at current estimate.
    Normal,
    /// Congestion detected, should decrease.
    Overuse,
}

/// Receiver-side bandwidth estimator.
pub struct BandwidthEstimator {
    /// Current bandwidth estimate in kbps.
    estimate_kbps: u32,
    /// Minimum allowed estimate.
    min_kbps: u32,
    /// Maximum allowed estimate.
    max_kbps: u32,
    /// Recent inter-arrival delay deltas (in microseconds).
    delay_deltas: Vec<i64>,
    /// Timestamps of received packets.
    last_arrival: Option<Instant>,
    /// Current state.
    state: BweState,
    /// Number of consecutive overuse detections.
    overuse_count: u32,
    /// Number of consecutive normal/underuse detections.
    normal_count: u32,
}

impl BandwidthEstimator {
    /// Create a new bandwidth estimator with default settings.
    pub fn new() -> Self {
        Self {
            estimate_kbps: INITIAL_BANDWIDTH_KBPS,
            min_kbps: MIN_BANDWIDTH_KBPS,
            max_kbps: MAX_BANDWIDTH_KBPS,
            delay_deltas: Vec::with_capacity(TREND_WINDOW_SIZE),
            last_arrival: None,
            state: BweState::Normal,
            overuse_count: 0,
            normal_count: 0,
        }
    }

    /// Create a new estimator with custom bandwidth range.
    pub fn with_range(min_kbps: u32, initial_kbps: u32, max_kbps: u32) -> Self {
        Self {
            estimate_kbps: initial_kbps.clamp(min_kbps, max_kbps),
            min_kbps,
            max_kbps,
            delay_deltas: Vec::with_capacity(TREND_WINDOW_SIZE),
            last_arrival: None,
            state: BweState::Normal,
            overuse_count: 0,
            normal_count: 0,
        }
    }

    /// Report a received packet. Call this for every received packet.
    pub fn on_packet_received(&mut self, _size_bytes: usize) {
        let now = Instant::now();

        if let Some(last) = self.last_arrival {
            let delta_us = now.duration_since(last).as_micros() as i64;
            self.delay_deltas.push(delta_us);

            if self.delay_deltas.len() > TREND_WINDOW_SIZE {
                self.delay_deltas.remove(0);
            }

            self.update_state();
        }

        self.last_arrival = Some(now);
    }

    /// Get the current bandwidth estimate in kbps.
    pub fn estimate_kbps(&self) -> u32 {
        self.estimate_kbps
    }

    /// Get the current state.
    pub fn state(&self) -> BweState {
        self.state
    }

    /// Manually set the estimate (e.g., from sender-side RTT feedback).
    pub fn set_estimate(&mut self, kbps: u32) {
        self.estimate_kbps = kbps.clamp(self.min_kbps, self.max_kbps);
    }

    /// Calculate the delay trend from recent samples.
    /// Returns a value between 0.0 (decreasing delay) and 1.0 (increasing delay).
    fn delay_trend(&self) -> f64 {
        if self.delay_deltas.len() < 3 {
            return 0.5;
        }

        let n = self.delay_deltas.len() as f64;
        let sum: f64 = self.delay_deltas.iter().map(|&d| d as f64).sum();
        let mean = sum / n;

        // Calculate slope of linear regression
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;
        for (i, &delta) in self.delay_deltas.iter().enumerate() {
            let x = i as f64 - n / 2.0;
            let y = delta as f64 - mean;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        if sum_x2 == 0.0 {
            return 0.5;
        }

        let slope = sum_xy / sum_x2;

        // Normalize slope to [0, 1] range
        // Positive slope = increasing delay = congestion
        
        (slope / 1000.0 + 0.5).clamp(0.0, 1.0)
    }

    /// Update the bandwidth estimate based on the current state.
    fn update_state(&mut self) {
        let trend = self.delay_trend();

        let new_state = if trend > OVERUSE_THRESHOLD {
            BweState::Overuse
        } else if trend < UNDERUSE_THRESHOLD {
            BweState::Underuse
        } else {
            BweState::Normal
        };

        match new_state {
            BweState::Overuse => {
                self.overuse_count += 1;
                self.normal_count = 0;

                // Only decrease after several consecutive overuse signals
                if self.overuse_count >= 3 {
                    // Multiplicative decrease
                    let factor = 0.85;
                    self.estimate_kbps =
                        ((self.estimate_kbps as f64 * factor) as u32).max(self.min_kbps);
                    self.overuse_count = 0;
                }
            }
            BweState::Underuse => {
                self.overuse_count = 0;
                self.normal_count += 1;

                // Additive increase
                if self.normal_count >= 5 {
                    let increase = (self.estimate_kbps as f64 * 0.05) as u32;
                    self.estimate_kbps =
                        (self.estimate_kbps + increase.max(50)).min(self.max_kbps);
                    self.normal_count = 0;
                }
            }
            BweState::Normal => {
                self.overuse_count = 0;
                self.normal_count += 1;
            }
        }

        self.state = new_state;
    }
}

impl Default for BandwidthEstimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bwe_new() {
        let bwe = BandwidthEstimator::new();
        assert_eq!(bwe.estimate_kbps(), INITIAL_BANDWIDTH_KBPS);
        assert_eq!(bwe.state(), BweState::Normal);
    }

    #[test]
    fn test_bwe_with_range() {
        let bwe = BandwidthEstimator::with_range(500, 3000, 10000);
        assert_eq!(bwe.estimate_kbps(), 3000);
    }

    #[test]
    fn test_bwe_with_range_clamped() {
        let bwe = BandwidthEstimator::with_range(500, 100, 10000);
        assert_eq!(bwe.estimate_kbps(), 500);
    }

    #[test]
    fn test_bwe_manual_set() {
        let mut bwe = BandwidthEstimator::new();
        bwe.set_estimate(5000);
        assert_eq!(bwe.estimate_kbps(), 5000);
    }

    #[test]
    fn test_bwe_manual_set_clamped() {
        let mut bwe = BandwidthEstimator::new();
        bwe.set_estimate(1);
        assert_eq!(bwe.estimate_kbps(), MIN_BANDWIDTH_KBPS);
        bwe.set_estimate(999999);
        assert_eq!(bwe.estimate_kbps(), MAX_BANDWIDTH_KBPS);
    }

    #[test]
    fn test_bwe_packets_received() {
        let mut bwe = BandwidthEstimator::new();
        // Simulate receiving packets
        bwe.on_packet_received(1000);
        assert_eq!(bwe.state(), BweState::Normal);
        bwe.on_packet_received(1000);
        bwe.on_packet_received(1000);
    }
}
