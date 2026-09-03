//! Adaptive encoding control policy.
//!
//! Pure decision logic (no I/O) driven by network observations: given RTT and
//! packet-loss measurements, produce encoder bitrate and capture/encode FPS
//! targets. Lives in `alldesk-core` — next to the transport glue that feeds it
//! and the codec that consumes it — so the control loop can be tested without
//! linking a video codec.

/// Adaptive bitrate controller that adjusts encoding quality based on
/// observed network conditions (RTT and packet loss).
///
/// Uses an additive-increase/multiplicative-decrease (AIMD) algorithm:
/// - On good conditions: slowly increase bitrate
/// - On congestion detected (high RTT or loss): rapidly decrease bitrate
pub struct AdaptiveBitrate {
    /// Current bitrate in kbps.
    current_bitrate_kbps: u32,
    /// Minimum allowed bitrate in kbps.
    min_bitrate_kbps: u32,
    /// Maximum allowed bitrate in kbps.
    max_bitrate_kbps: u32,
    /// Target bitrate we're converging towards.
    target_bitrate_kbps: u32,
    /// Additive increase step in kbps (applied per update when conditions are good).
    increase_step_kbps: u32,
    /// Multiplicative decrease factor (e.g., 0.7 means reduce to 70%).
    decrease_factor: f64,
    /// RTT threshold in ms above which we consider congestion.
    rtt_threshold_ms: f64,
    /// Loss rate threshold (0.0 - 1.0) above which we consider congestion.
    loss_threshold: f64,
    /// Number of consecutive good observations before increasing.
    good_observations_needed: u32,
    /// Current count of consecutive good observations.
    good_observations: u32,
}

impl AdaptiveBitrate {
    pub fn new(initial_bitrate_kbps: u32, min_bitrate_kbps: u32, max_bitrate_kbps: u32) -> Self {
        Self {
            current_bitrate_kbps: initial_bitrate_kbps,
            min_bitrate_kbps,
            max_bitrate_kbps,
            target_bitrate_kbps: initial_bitrate_kbps,
            increase_step_kbps: 100,
            decrease_factor: 0.7,
            rtt_threshold_ms: 100.0,
            loss_threshold: 0.02,
            good_observations_needed: 5,
            good_observations: 0,
        }
    }

    /// Report observed network conditions. Returns the recommended bitrate.
    pub fn update(&mut self, rtt_ms: f64, loss_rate: f64) -> u32 {
        let congested = rtt_ms > self.rtt_threshold_ms || loss_rate > self.loss_threshold;

        if congested {
            // Multiplicative decrease
            self.target_bitrate_kbps = ((self.target_bitrate_kbps as f64 * self.decrease_factor)
                as u32)
                .max(self.min_bitrate_kbps);
            self.good_observations = 0;
        } else {
            self.good_observations += 1;
            if self.good_observations >= self.good_observations_needed {
                // Additive increase
                self.target_bitrate_kbps =
                    (self.target_bitrate_kbps + self.increase_step_kbps).min(self.max_bitrate_kbps);
                self.good_observations = 0;
            }
        }

        self.current_bitrate_kbps = self.target_bitrate_kbps;
        self.current_bitrate_kbps
    }

    /// Get the current recommended bitrate.
    pub fn current_bitrate(&self) -> u32 {
        self.current_bitrate_kbps
    }

    /// Manually set the bitrate (e.g., from user preference).
    pub fn set_bitrate(&mut self, bitrate_kbps: u32) {
        let clamped = bitrate_kbps.clamp(self.min_bitrate_kbps, self.max_bitrate_kbps);
        self.current_bitrate_kbps = clamped;
        self.target_bitrate_kbps = clamped;
    }

    /// Set the congestion detection thresholds.
    pub fn set_thresholds(&mut self, rtt_ms: f64, loss_rate: f64) {
        self.rtt_threshold_ms = rtt_ms;
        self.loss_threshold = loss_rate;
    }
}

/// Adaptive frame rate controller that adjusts capture/encode FPS based on
/// network conditions. When bandwidth is constrained, the frame rate is
/// reduced before quality to maintain interactivity.
pub struct AdaptiveFramerate {
    /// Current target FPS.
    current_fps: u32,
    /// Minimum allowed FPS.
    min_fps: u32,
    /// Maximum allowed FPS.
    max_fps: u32,
    /// RTT threshold for congestion.
    rtt_threshold_ms: f64,
    /// Loss threshold for congestion.
    loss_threshold: f64,
}

impl AdaptiveFramerate {
    pub fn new(min_fps: u32, max_fps: u32) -> Self {
        Self {
            current_fps: max_fps,
            min_fps,
            max_fps,
            rtt_threshold_ms: 150.0,
            loss_threshold: 0.05,
        }
    }

    /// Update frame rate based on observed network conditions.
    /// Returns the recommended FPS.
    pub fn update(&mut self, rtt_ms: f64, loss_rate: f64) -> u32 {
        let congested = rtt_ms > self.rtt_threshold_ms || loss_rate > self.loss_threshold;

        if congested {
            // Drop FPS by ~25% on congestion
            let new_fps = (self.current_fps as f64 * 0.75) as u32;
            self.current_fps = new_fps.max(self.min_fps);
        } else if self.current_fps < self.max_fps {
            // Gradually restore FPS when conditions improve
            let new_fps = self.current_fps + 2;
            self.current_fps = new_fps.min(self.max_fps);
        }

        self.current_fps
    }

    /// Get current FPS.
    pub fn current_fps(&self) -> u32 {
        self.current_fps
    }

    /// Set thresholds for congestion detection.
    pub fn set_thresholds(&mut self, rtt_ms: f64, loss_rate: f64) {
        self.rtt_threshold_ms = rtt_ms;
        self.loss_threshold = loss_rate;
    }
}

/// Encoder/pacing targets produced by [`AdaptiveController`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptiveTargets {
    pub bitrate_kbps: u32,
    pub fps: u32,
}

/// One control loop combining the AIMD bitrate and framerate policies: feed
/// it per-interval network observations, apply the returned targets to the
/// encoder and capture pacing.
///
/// The two policies use slightly different congestion thresholds on purpose:
/// framerate has looser thresholds so bandwidth problems first reduce FPS
/// (keeping interactivity) and only hit the bitrate when things get worse.
pub struct AdaptiveController {
    bitrate: AdaptiveBitrate,
    framerate: AdaptiveFramerate,
    targets: AdaptiveTargets,
}

impl AdaptiveController {
    pub fn new(
        initial_bitrate_kbps: u32,
        min_bitrate_kbps: u32,
        max_bitrate_kbps: u32,
        min_fps: u32,
        max_fps: u32,
    ) -> Self {
        let initial_bitrate_kbps = initial_bitrate_kbps.clamp(min_bitrate_kbps, max_bitrate_kbps);
        // Never let a degenerate min_fps stall the capture loop entirely.
        let min_fps = min_fps.max(1);
        Self {
            bitrate: AdaptiveBitrate::new(initial_bitrate_kbps, min_bitrate_kbps, max_bitrate_kbps),
            framerate: AdaptiveFramerate::new(min_fps, max_fps.max(min_fps)),
            targets: AdaptiveTargets {
                bitrate_kbps: initial_bitrate_kbps,
                fps: max_fps,
            },
        }
    }

    /// Feed one observation; returns the targets to apply now.
    pub fn update(&mut self, rtt_ms: f64, loss_rate: f64) -> AdaptiveTargets {
        let bitrate_kbps = self.bitrate.update(rtt_ms, loss_rate);
        let fps = self.framerate.update(rtt_ms, loss_rate);
        self.targets = AdaptiveTargets { bitrate_kbps, fps };
        self.targets
    }

    /// Targets most recently returned by [`update`](Self::update).
    pub fn targets(&self) -> AdaptiveTargets {
        self.targets
    }
}

/// Converts cumulative transport packet counters into a per-interval loss
/// rate. Transport stats are monotonic totals, so the rate must be computed
/// from deltas — using the raw totals would make loss look permanent after a
/// single bad moment.
pub struct LossRateTracker {
    last_sent: u64,
    last_lost: u64,
}

impl Default for LossRateTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl LossRateTracker {
    pub fn new() -> Self {
        Self {
            last_sent: 0,
            last_lost: 0,
        }
    }

    /// Feed cumulative sent/lost packet counts; returns the loss rate over
    /// the interval since the previous call. Counter resets (new connection)
    /// are handled by saturating subtraction.
    pub fn update(&mut self, sent_total: u64, lost_total: u64) -> f64 {
        let sent_delta = sent_total.saturating_sub(self.last_sent);
        let lost_delta = lost_total.saturating_sub(self.last_lost);
        self.last_sent = sent_total;
        self.last_lost = lost_total;
        if sent_delta == 0 {
            0.0
        } else {
            (lost_delta as f64 / sent_delta as f64).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_bitrate_starts_at_initial() {
        let abr = AdaptiveBitrate::new(2000, 500, 8000);
        assert_eq!(abr.current_bitrate(), 2000);
    }

    #[test]
    fn test_adaptive_bitrate_increase_on_good_conditions() {
        let mut abr = AdaptiveBitrate::new(2000, 500, 8000);

        // Need good_observations_needed consecutive good updates
        for _ in 0..5 {
            abr.update(50.0, 0.0); // Good RTT and no loss
        }

        assert!(abr.current_bitrate() > 2000);
    }

    #[test]
    fn test_adaptive_bitrate_decrease_on_congestion() {
        let mut abr = AdaptiveBitrate::new(2000, 500, 8000);

        // Simulate congestion
        let new_bitrate = abr.update(200.0, 0.05);

        assert!(new_bitrate < 2000);
        assert!(new_bitrate >= 500); // Min bitrate
    }

    #[test]
    fn test_adaptive_bitrate_respects_min_max() {
        let mut abr = AdaptiveBitrate::new(2000, 500, 3000);

        // Try to go above max
        for _ in 0..100 {
            abr.update(10.0, 0.0);
        }
        assert!(abr.current_bitrate() <= 3000);

        // Try to go below min
        for _ in 0..100 {
            abr.update(500.0, 0.5);
        }
        assert!(abr.current_bitrate() >= 500);
    }

    #[test]
    fn test_adaptive_bitrate_manual_set() {
        let mut abr = AdaptiveBitrate::new(2000, 500, 8000);
        abr.set_bitrate(4000);
        assert_eq!(abr.current_bitrate(), 4000);
    }

    #[test]
    fn test_adaptive_bitrate_manual_set_clamped() {
        let mut abr = AdaptiveBitrate::new(2000, 500, 8000);
        abr.set_bitrate(100); // Below min
        assert_eq!(abr.current_bitrate(), 500);
        abr.set_bitrate(99999); // Above max
        assert_eq!(abr.current_bitrate(), 8000);
    }

    #[test]
    fn test_adaptive_framerate_starts_at_max() {
        let afr = AdaptiveFramerate::new(5, 30);
        assert_eq!(afr.current_fps(), 30);
    }

    #[test]
    fn test_adaptive_framerate_decrease_on_congestion() {
        let mut afr = AdaptiveFramerate::new(5, 30);
        let fps = afr.update(200.0, 0.1);
        assert!(fps < 30);
        assert!(fps >= 5);
    }

    #[test]
    fn test_adaptive_framerate_increase_on_good_conditions() {
        let mut afr = AdaptiveFramerate::new(5, 30);
        // Drop FPS first
        afr.update(200.0, 0.1);
        let reduced = afr.current_fps();
        assert!(reduced < 30);
        // Good conditions should restore FPS
        for _ in 0..20 {
            afr.update(30.0, 0.0);
        }
        assert!(afr.current_fps() > reduced);
    }

    #[test]
    fn test_adaptive_framerate_respects_min_max() {
        let mut afr = AdaptiveFramerate::new(5, 30);
        // Hammer with congestion
        for _ in 0..50 {
            afr.update(500.0, 0.5);
        }
        assert!(afr.current_fps() >= 5);

        // Hammer with good conditions
        for _ in 0..50 {
            afr.update(10.0, 0.0);
        }
        assert!(afr.current_fps() <= 30);
    }

    // ---- AdaptiveController ----

    #[test]
    fn test_controller_congestion_reduces_both_targets() {
        let mut ctrl = AdaptiveController::new(4000, 500, 8000, 5, 30);
        let t = ctrl.update(300.0, 0.1);
        assert!(t.bitrate_kbps < 4000);
        assert!(t.fps < 30);
        assert_eq!(ctrl.targets(), t);
    }

    #[test]
    fn test_controller_recovery_raises_targets() {
        let mut ctrl = AdaptiveController::new(4000, 500, 8000, 5, 30);
        let worst = ctrl.update(300.0, 0.1);

        // Mild conditions stay below the framerate threshold (150 ms) but
        // above the bitrate threshold (100 ms): FPS recovers while the
        // bitrate keeps shrinking.
        let lowest = ctrl.update(120.0, 0.0);
        assert!(lowest.fps > worst.fps);
        assert!(lowest.bitrate_kbps < worst.bitrate_kbps);

        // Fully good conditions recover both from the lowest point.
        for _ in 0..30 {
            ctrl.update(20.0, 0.0);
        }
        let recovered = ctrl.targets();
        assert_eq!(recovered.fps, 30);
        assert!(recovered.bitrate_kbps > lowest.bitrate_kbps);
    }

    #[test]
    fn test_controller_targets_stay_in_bounds() {
        let mut ctrl = AdaptiveController::new(4000, 500, 8000, 5, 30);
        for _ in 0..100 {
            let t = ctrl.update(1000.0, 1.0);
            assert!(t.bitrate_kbps >= 500);
            assert!(t.bitrate_kbps <= 8000);
            assert!(t.fps >= 5);
            assert!(t.fps <= 30);
        }
        for _ in 0..100 {
            let t = ctrl.update(1.0, 0.0);
            assert!(t.bitrate_kbps <= 8000);
            assert!(t.fps <= 30);
        }
    }

    #[test]
    fn test_controller_initial_bitrate_clamped_and_fps_min_one() {
        let mut ctrl = AdaptiveController::new(99_999, 500, 8000, 0, 30);
        assert_eq!(ctrl.targets().bitrate_kbps, 8000);
        // Degenerate min_fps=0 must not produce a 0 FPS target (division by
        // zero in capture pacing).
        let t = ctrl.update(1000.0, 1.0);
        assert!(t.fps >= 1);
    }

    // ---- LossRateTracker ----

    #[test]
    fn test_loss_rate_tracker_computes_delta_rate() {
        let mut trk = LossRateTracker::new();
        // First interval: 100 sent, 5 lost → 5%
        assert!((trk.update(100, 5) - 0.05).abs() < 1e-9);
        // Next interval: 200 more sent, 0 more lost → 0%
        assert!(trk.update(300, 5) == 0.0);
    }

    #[test]
    fn test_loss_rate_tracker_no_traffic_is_zero() {
        let mut trk = LossRateTracker::new();
        assert_eq!(trk.update(0, 0), 0.0);
        assert_eq!(trk.update(100, 3), 0.03);
        assert_eq!(trk.update(100, 3), 0.0); // idle interval
    }

    #[test]
    fn test_loss_rate_counter_reset_has_no_phantom_spike() {
        let mut trk = LossRateTracker::new();
        assert_eq!(trk.update(1000, 100), 0.1);
        // A counter reset (new connection) reports totals below the previous
        // ones; saturating subtraction must yield a zero delta — a phantom
        // 100% loss interval right after a reconnect would slam the AIMD
        // controller to minimum bitrate for no reason.
        assert_eq!(trk.update(50, 2), 0.0);
        // Normal tracking resumes from the new baseline.
        assert!((trk.update(150, 5) - 0.03).abs() < 1e-9);
    }

    #[test]
    fn test_loss_rate_tracker_clamps_above_one() {
        let mut trk = LossRateTracker::new();
        assert_eq!(trk.update(10, 100), 1.0);
    }
}
