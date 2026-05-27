use alldesk_core::Result;
use alldesk_capture::capture::CapturedFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    VP9,
    H264,
    AV1,
}

pub struct EncodedPacket {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
    pub timestamp_ms: u64,
    pub codec: Codec,
}

pub trait VideoEncoder: Send + Sync {
    fn encode(&mut self, frame: &CapturedFrame) -> Result<Vec<EncodedPacket>>;
    fn set_bitrate(&mut self, bitrate_kbps: u32);
    fn request_key_frame(&mut self);
    fn codec(&self) -> Codec;
}

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
            self.target_bitrate_kbps = ((self.target_bitrate_kbps as f64 * self.decrease_factor) as u32)
                .max(self.min_bitrate_kbps);
            self.good_observations = 0;
        } else {
            self.good_observations += 1;
            if self.good_observations >= self.good_observations_needed {
                // Additive increase
                self.target_bitrate_kbps = (self.target_bitrate_kbps + self.increase_step_kbps)
                    .min(self.max_bitrate_kbps);
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
}
