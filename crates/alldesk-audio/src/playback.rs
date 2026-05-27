use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use alldesk_core::{Error, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

/// Plays audio to the default output device at 48 kHz mono f32.
///
/// Call `play(data)` to queue raw f32 sample bytes for playback.
/// The output stream runs continuously once created.
pub struct AudioPlayer {
    /// The active cpal output stream.
    stream: Option<Stream>,
    /// Shared sample buffer that the cpal callback reads from.
    buffer: Arc<Mutex<VecDeque<f32>>>,
}

unsafe impl Send for AudioPlayer {}
unsafe impl Sync for AudioPlayer {}

impl AudioPlayer {
    /// Create a new player. The output stream is opened immediately.
    pub fn new() -> Result<Self> {
        let buffer = Arc::new(Mutex::new(VecDeque::<f32>::new()));

        let stream = Self::build_stream(&buffer)?;

        Ok(Self {
            stream: Some(stream),
            buffer,
        })
    }

    /// Build the cpal output stream for 48 kHz f32 playback.
    fn build_stream(buffer: &Arc<Mutex<VecDeque<f32>>>) -> Result<Stream> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| Error::Audio("No default output device available".into()))?;

        // Try to find a supported config matching 48 kHz, f32, >=1 channel.
        let supported = device
            .supported_output_configs()
            .map_err(|e| Error::Audio(format!("Failed to query output configs: {e}")))?;

        let use_config = supported
            .filter(|c| c.channels() >= 1)
            .filter(|c| c.min_sample_rate() <= 48_000 && c.max_sample_rate() >= 48_000)
            .find(|c| c.sample_format() == SampleFormat::F32)
            .map(|c| {
                // Keep the device's native channel count; the callback will
                // duplicate mono samples to fill all channels.
                c.with_sample_rate(48_000).config()
            })
            .unwrap_or_else(|| {
                tracing::warn!(
                    "Could not find exact 48 kHz f32 output config, using fallback"
                );
                StreamConfig {
                    channels: 1,
                    sample_rate: 48_000,
                    buffer_size: cpal::BufferSize::Default,
                }
            });

        let channels = use_config.channels as usize;
        let buf = buffer.clone();

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

        // Limit the buffer to ~0.5 seconds at 48 kHz to prevent unbounded
        // growth if data arrives faster than it is consumed.
        const MAX_BUFFER_SAMPLES: usize = 24_000; // 48000 / 2
        if guard.len() + samples.len() > MAX_BUFFER_SAMPLES {
            let excess = guard.len() + samples.len() - MAX_BUFFER_SAMPLES;
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
