//! Frame pipeline latency measurement and analysis.
//!
//! Instruments the capture → encode → transport → decode pipeline
//! to measure end-to-end latency at each stage.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Maximum number of latency samples to keep per stage.
const MAX_SAMPLES: usize = 120;

/// A latency measurement for a single pipeline stage.
#[derive(Debug, Clone)]
pub struct LatencySample {
    /// Timestamp when the frame entered this stage.
    pub enter_us: u64,
    /// Timestamp when the frame left this stage.
    pub exit_us: u64,
    /// Duration in microseconds.
    pub duration_us: u64,
}

/// Latency statistics for a pipeline stage.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LatencyStats {
    /// Stage name.
    pub stage: String,
    /// Number of samples collected.
    pub sample_count: usize,
    /// Minimum latency in microseconds.
    pub min_us: u64,
    /// Maximum latency in microseconds.
    pub max_us: u64,
    /// Average latency in microseconds.
    pub avg_us: u64,
    /// 50th percentile (median) latency in microseconds.
    pub p50_us: u64,
    /// 95th percentile latency in microseconds.
    pub p95_us: u64,
    /// 99th percentile latency in microseconds.
    pub p99_us: u64,
}

/// Tracks latency measurements for a single pipeline stage.
pub struct StageTimer {
    /// Stage name.
    stage: String,
    /// Monotonically increasing counter for correlating start/end.
    counter: AtomicU64,
    /// Pending start times keyed by counter value.
    pending: parking_lot::Mutex<VecDeque<(u64, u64)>>,
    /// Completed latency samples.
    samples: parking_lot::Mutex<VecDeque<LatencySample>>,
}

impl StageTimer {
    pub fn new(stage: &str) -> Self {
        Self {
            stage: stage.to_string(),
            counter: AtomicU64::new(0),
            pending: parking_lot::Mutex::new(VecDeque::new()),
            samples: parking_lot::Mutex::new(VecDeque::with_capacity(MAX_SAMPLES)),
        }
    }

    /// Start a timing measurement. Returns a unique ID to pass to `end()`.
    pub fn start(&self) -> u64 {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        let now = micros_since_epoch();
        self.pending.lock().push_back((id, now));
        id
    }

    /// End a timing measurement previously started with `start()`.
    pub fn end(&self, id: u64) {
        let now = micros_since_epoch();
        let mut pending = self.pending.lock();
        if let Some(pos) = pending.iter().position(|(i, _)| *i == id) {
            if let Some((_, enter_us)) = pending.remove(pos) {
                let sample = LatencySample {
                    enter_us,
                    exit_us: now,
                    duration_us: now.saturating_sub(enter_us),
                };
                let mut samples = self.samples.lock();
                if samples.len() >= MAX_SAMPLES {
                    samples.pop_front();
                }
                samples.push_back(sample);
            }
        }
    }

    /// Compute latency statistics from collected samples.
    pub fn stats(&self) -> LatencyStats {
        let samples = self.samples.lock();
        let n = samples.len();

        if n == 0 {
            return LatencyStats {
                stage: self.stage.clone(),
                sample_count: 0,
                min_us: 0,
                max_us: 0,
                avg_us: 0,
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            };
        }

        let mut durations: Vec<u64> = samples.iter().map(|s| s.duration_us).collect();
        durations.sort();

        let min = durations[0];
        let max = durations[n - 1];
        let avg = durations.iter().sum::<u64>() / n as u64;
        let p50 = durations[(n - 1) * 50 / 100];
        let p95 = durations[(n - 1) * 95 / 100];
        let p99 = durations[(n - 1) * 99 / 100];

        LatencyStats {
            stage: self.stage.clone(),
            sample_count: n,
            min_us: min,
            max_us: max,
            avg_us: avg,
            p50_us: p50,
            p95_us: p95,
            p99_us: p99,
        }
    }

    /// Get the number of collected samples.
    pub fn sample_count(&self) -> usize {
        self.samples.lock().len()
    }

    /// Clear all samples.
    pub fn reset(&self) {
        self.samples.lock().clear();
        self.pending.lock().clear();
    }
}

/// Pipeline latency tracker that measures all stages of the frame pipeline.
pub struct PipelineLatencyTracker {
    pub capture: StageTimer,
    pub encode: StageTimer,
    pub transport_send: StageTimer,
    pub transport_recv: StageTimer,
    pub decode: StageTimer,
}

impl PipelineLatencyTracker {
    pub fn new() -> Self {
        Self {
            capture: StageTimer::new("capture"),
            encode: StageTimer::new("encode"),
            transport_send: StageTimer::new("transport_send"),
            transport_recv: StageTimer::new("transport_recv"),
            decode: StageTimer::new("decode"),
        }
    }

    /// Compute end-to-end latency stats by summing all stages.
    pub fn end_to_end_stats(&self) -> LatencyStats {
        let all_stats = [
            self.capture.stats(),
            self.encode.stats(),
            self.transport_send.stats(),
            self.transport_recv.stats(),
            self.decode.stats(),
        ];

        let n = all_stats.iter().map(|s| s.sample_count).min().unwrap_or(0);
        if n == 0 {
            return LatencyStats {
                stage: "end_to_end".to_string(),
                sample_count: 0,
                min_us: 0,
                max_us: 0,
                avg_us: 0,
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
            };
        }

        let avg_us: u64 = all_stats.iter().map(|s| s.avg_us).sum();
        let min_us: u64 = all_stats.iter().map(|s| s.min_us).sum();
        let max_us: u64 = all_stats.iter().map(|s| s.max_us).sum();
        let p50_us: u64 = all_stats.iter().map(|s| s.p50_us).sum();
        let p95_us: u64 = all_stats.iter().map(|s| s.p95_us).sum();
        let p99_us: u64 = all_stats.iter().map(|s| s.p99_us).sum();

        LatencyStats {
            stage: "end_to_end".to_string(),
            sample_count: n,
            min_us,
            max_us,
            avg_us,
            p50_us,
            p95_us,
            p99_us,
        }
    }
}

impl Default for PipelineLatencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current time in microseconds using a monotonic clock.
fn micros_since_epoch() -> u64 {
    let now = Instant::now();
    // Use a reference point to get relative microseconds.
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    now.duration_since(*start).as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_timer_basic() {
        let timer = StageTimer::new("test");
        let id = timer.start();
        timer.end(id);
        assert_eq!(timer.sample_count(), 1);

        let stats = timer.stats();
        assert_eq!(stats.stage, "test");
        assert_eq!(stats.sample_count, 1);
        assert!(stats.avg_us < 10000); // Should be very fast
    }

    #[test]
    fn test_stage_timer_multiple_samples() {
        let timer = StageTimer::new("multi");
        for _ in 0..10 {
            let id = timer.start();
            std::thread::sleep(std::time::Duration::from_micros(100));
            timer.end(id);
        }
        assert_eq!(timer.sample_count(), 10);

        let stats = timer.stats();
        assert!(stats.min_us <= stats.avg_us);
        assert!(stats.avg_us <= stats.max_us);
        assert!(stats.p50_us >= stats.min_us);
        assert!(stats.p95_us >= stats.p50_us);
    }

    #[test]
    fn test_stage_timer_reset() {
        let timer = StageTimer::new("reset");
        let id = timer.start();
        timer.end(id);
        assert_eq!(timer.sample_count(), 1);

        timer.reset();
        assert_eq!(timer.sample_count(), 0);
    }

    #[test]
    fn test_stage_timer_empty_stats() {
        let timer = StageTimer::new("empty");
        let stats = timer.stats();
        assert_eq!(stats.sample_count, 0);
        assert_eq!(stats.avg_us, 0);
    }

    #[test]
    fn test_pipeline_tracker() {
        let tracker = PipelineLatencyTracker::new();

        let id1 = tracker.capture.start();
        tracker.capture.end(id1);

        let id2 = tracker.encode.start();
        tracker.encode.end(id2);

        let id3 = tracker.transport_send.start();
        tracker.transport_send.end(id3);

        let id4 = tracker.transport_recv.start();
        tracker.transport_recv.end(id4);

        let id5 = tracker.decode.start();
        tracker.decode.end(id5);

        let e2e = tracker.end_to_end_stats();
        assert_eq!(e2e.sample_count, 1);
        assert!(e2e.avg_us >= 0);
    }

    #[test]
    fn test_stage_timer_max_samples() {
        let timer = StageTimer::new("overflow");
        for _ in 0..MAX_SAMPLES + 10 {
            let id = timer.start();
            timer.end(id);
        }
        assert_eq!(timer.sample_count(), MAX_SAMPLES);
    }

    #[test]
    fn test_latency_stats_serialization() {
        let stats = LatencyStats {
            stage: "test".to_string(),
            sample_count: 10,
            min_us: 100,
            max_us: 500,
            avg_us: 250,
            p50_us: 240,
            p95_us: 450,
            p99_us: 490,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"stage\":\"test\""));
        assert!(json.contains("\"avg_us\":250"));
    }
}
