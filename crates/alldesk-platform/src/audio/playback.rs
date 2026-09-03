use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alldesk_core::{Error, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

/// Number of samples buffered before playback starts consuming (~60 ms of
/// audio at the stream's rate). Without this prefill the buffer starts empty
/// and the very first network jitter is rendered as an audible gap.
fn prefill_samples(sample_rate: u32) -> usize {
    (sample_rate as usize * 60 / 1000).max(1)
}

/// Plays audio to the default output device at 48 kHz mono f32.
///
/// Call `play(data)` to queue raw f32 sample bytes for playback.
/// The output stream runs continuously once created.
pub struct AudioPlayer {
    /// The active cpal output stream.
    stream: Option<Stream>,
    /// Shared sample buffer that the cpal callback reads from.
    buffer: Arc<Mutex<VecDeque<f32>>>,
    /// Configured input sample rate, used to size the jitter buffer.
    sample_rate: u32,
}

unsafe impl Send for AudioPlayer {}
unsafe impl Sync for AudioPlayer {}

impl AudioPlayer {
    /// Create a new player at the default 48 kHz. The output stream is
    /// opened immediately.
    pub fn new() -> Result<Self> {
        Self::with_sample_rate(48_000)
    }

    /// Create a new player running at `sample_rate` Hz f32 mono input
    /// (the input data's rate, not necessarily the device's native rate —
    /// cpal resamples via its fallback path or the device is configured).
    pub fn with_sample_rate(sample_rate: u32) -> Result<Self> {
        let buffer = Arc::new(Mutex::new(VecDeque::<f32>::new()));

        let stream = Self::build_stream(&buffer, sample_rate)?;

        Ok(Self {
            stream: Some(stream),
            buffer,
            sample_rate,
        })
    }

    /// Build the cpal output stream for f32 playback at `sample_rate`.
    fn build_stream(buffer: &Arc<Mutex<VecDeque<f32>>>, sample_rate: u32) -> Result<Stream> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| Error::Audio("No default output device available".into()))?;

        // Try to find a supported config matching the requested rate, f32, >=1 channel.
        let supported = device
            .supported_output_configs()
            .map_err(|e| Error::Audio(format!("Failed to query output configs: {e}")))?;

        let use_config = supported
            .filter(|c| c.channels() >= 1)
            .filter(|c| c.min_sample_rate() <= sample_rate && c.max_sample_rate() >= sample_rate)
            .find(|c| c.sample_format() == SampleFormat::F32)
            .map(|c| {
                // Keep the device's native channel count; the callback will
                // duplicate mono samples to fill all channels.
                c.with_sample_rate(sample_rate).config()
            })
            .unwrap_or_else(|| {
                tracing::warn!(
                    "Could not find exact {} Hz f32 output config, using fallback",
                    sample_rate
                );
                StreamConfig {
                    channels: 1,
                    sample_rate,
                    buffer_size: cpal::BufferSize::Default,
                }
            });

        let channels = use_config.channels as usize;
        let buf = buffer.clone();
        // Flips to true once the jitter buffer has been filled for the first
        // time; playback then runs without the prefill gate (underruns fall
        // back to per-sample silence, as before).
        let started = Arc::new(AtomicBool::new(false));
        let prefill_target = prefill_samples(sample_rate);

        let stream = device
            .build_output_stream(
                &use_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut guard = match buf.lock() {
                        Ok(g) => g,
                        Err(_) => {
                            // Poisoned — fill with silence.
                            data.fill(0.0);
                            return;
                        }
                    };

                    if !started.load(Ordering::Relaxed) {
                        if guard.len() < prefill_target {
                            // Still filling the jitter buffer — output silence
                            // instead of consuming the samples we're saving up.
                            data.fill(0.0);
                            return;
                        }
                        started.store(true, Ordering::Relaxed);
                    }

                    if channels == 1 {
                        // Simple mono: one sample per frame.
                        for sample in data.iter_mut() {
                            *sample = guard.pop_front().unwrap_or(0.0);
                        }
                    } else {
                        // Multi-channel: duplicate mono sample to all channels.
                        for frame in data.chunks_mut(channels) {
                            let s = guard.pop_front().unwrap_or(0.0);
                            for ch in frame.iter_mut() {
                                *ch = s;
                            }
                        }
                    }
                },
                |err| {
                    tracing::error!("Audio playback stream error: {err}");
                },
                None,
            )
            .map_err(|e| Error::Audio(format!("Failed to build output stream: {e}")))?;

        stream
            .play()
            .map_err(|e| Error::Audio(format!("Failed to start output stream: {e}")))?;

        tracing::info!(
            "Audio playback started: {} Hz, {} ch, f32",
            use_config.sample_rate,
            use_config.channels
        );

        Ok(stream)
    }

    /// Queue raw audio data for playback.
    ///
    /// `data` must be a slice of little-endian f32 bytes (i.e. the raw byte
    /// representation of `&[f32]`). Samples are appended to an internal
    /// buffer and consumed by the output stream callback.
    pub fn play(&self, data: &[u8]) -> Result<()> {
        if !data.len().is_multiple_of(4) {
            return Err(Error::Audio(format!(
                "Audio data length ({}) is not a multiple of 4 (f32 size)",
                data.len()
            )));
        }

        let samples: Vec<f32> = data
            .chunks_exact(4)
            .map(|chunk| {
                let bytes: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                f32::from_le_bytes(bytes)
            })
            .collect();

        let mut guard = self
            .buffer
            .lock()
            .map_err(|_| Error::Audio("Playback buffer lock poisoned".into()))?;

        // Limit the buffer to ~0.5 seconds to prevent unbounded growth if
        // data arrives faster than it is consumed.
        let max_buffer_samples = (self.sample_rate as usize / 2).max(1);
        if guard.len() + samples.len() > max_buffer_samples {
            let excess = guard.len() + samples.len() - max_buffer_samples;
            guard.drain(..excess);
        }

        guard.extend(samples);
        Ok(())
    }

    /// Stop the output stream and clear the buffer.
    pub fn stop(&mut self) -> Result<()> {
        if let Some(stream) = self.stream.take() {
            drop(stream);
            tracing::info!("Audio playback stopped");
        }
        if let Ok(mut guard) = self.buffer.lock() {
            guard.clear();
        }
        Ok(())
    }

    /// Returns true if the player currently has an active stream.
    pub fn is_playing(&self) -> bool {
        self.stream.is_some()
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefill_samples_is_about_60ms() {
        assert_eq!(prefill_samples(48_000), 2880);
        assert_eq!(prefill_samples(44_100), 2646);
        assert_eq!(prefill_samples(16_000), 960);
        // Degenerate rates must still yield at least one sample.
        assert_eq!(prefill_samples(0), 1);
        assert_eq!(prefill_samples(1), 1);
    }

    #[test]
    fn test_player_play_invalid_data() {
        let player = AudioPlayer::new();
        // May fail if no audio device, which is fine
        if let Ok(player) = player {
            // Data length not multiple of 4 should fail
            let result = player.play(&[1, 2, 3]);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_player_play_valid_data() {
        let player = AudioPlayer::new();
        if let Ok(player) = player {
            // Valid f32 data (4 bytes)
            let sample = 0.5f32.to_le_bytes();
            let result = player.play(&sample);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_player_is_playing() {
        let player = AudioPlayer::new();
        if let Ok(player) = player {
            assert!(player.is_playing());
        }
    }

    #[test]
    fn test_player_stop() {
        let player = AudioPlayer::new();
        if let Ok(mut player) = player {
            player.stop().unwrap();
            assert!(!player.is_playing());
        }
    }
}
