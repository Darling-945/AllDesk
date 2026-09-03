use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use alldesk_core::{Error, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

/// Captures audio from the default input device at 48 kHz mono f32.
///
/// Call `start()` to begin capture and `recv_chunk()` to retrieve raw f32
/// sample buffers as `Vec<u8>`.
pub struct AudioCapturer {
    /// The active cpal input stream. `None` when stopped.
    stream: Option<Stream>,
    /// Sender half — the cpal callback writes into this.
    tx: mpsc::SyncSender<Vec<u8>>,
    /// Receiver half — `recv_chunk` reads from this.
    rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
    /// Sample rate of the active stream (0 before `start()`). The receiver
    /// needs this to play chunks at the right speed.
    sample_rate: u32,
}

// The cpal `Stream` is `Send` but not `Sync`; we only ever access it from
// the struct itself (single-threaded usage model), so this is safe.
unsafe impl Send for AudioCapturer {}
unsafe impl Sync for AudioCapturer {}

impl AudioCapturer {
    /// Create a new capturer. No stream is opened until `start()` is called.
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(16);
        Ok(Self {
            stream: None,
            tx,
            rx: Arc::new(Mutex::new(rx)),
            sample_rate: 0,
        })
    }

    /// The sample rate of the active capture stream, or 0 if not started.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Open the default input device and start capturing at 48 kHz mono f32.
    pub fn start(&mut self) -> Result<()> {
        // If already running, stop the previous stream first.
        self.stop()?;

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| Error::Audio("No default input device available".into()))?;

        // Try to find a supported config matching 48 kHz, f32, >=1 channel.
        let supported = device
            .supported_input_configs()
            .map_err(|e| Error::Audio(format!("Failed to query input configs: {e}")))?;

        // Preferred sample rates to try, in order of preference.
        let preferred_rates: &[u32] = &[48_000, 44_100, 16_000];

        // Try to find an exact match for preferred rate + f32 format first.
        let use_config = preferred_rates
            .iter()
            .find_map(|&rate| {
                supported
                    .clone()
                    .filter(|c| c.channels() >= 1)
                    .filter(|c| c.min_sample_rate() <= rate && c.max_sample_rate() >= rate)
                    .find(|c| c.sample_format() == SampleFormat::F32)
                    .map(|c| {
                        let mut cfg: StreamConfig = c.with_sample_rate(rate).config();
                        cfg.channels = 1;
                        cfg
                    })
            })
            .or_else(|| {
                // Fall back to any supported f32 config at the minimum sample rate.
                tracing::warn!("No preferred sample rate available, trying any f32 config");
                supported
                    .clone()
                    .filter(|c| c.channels() >= 1)
                    .find(|c| c.sample_format() == SampleFormat::F32)
                    .map(|c| {
                        let rate = c.min_sample_rate();
                        let mut cfg: StreamConfig = c.with_sample_rate(rate).config();
                        cfg.channels = 1;
                        cfg
                    })
            })
            .or_else(|| {
                // Last resort: try any supported config with any sample format.
                tracing::warn!("No f32 config available, trying any supported config");
                supported
                    .clone().find(|c| c.channels() >= 1)
                    .map(|c| {
                        let rate = c.min_sample_rate();
                        let mut cfg: StreamConfig = c.with_sample_rate(rate).config();
                        cfg.channels = 1;
                        cfg
                    })
            });

        let use_config = match use_config {
            Some(cfg) => cfg,
            None => {
                return Err(Error::Audio(
                    "No supported audio input configuration found".into(),
                ));
            }
        };

        let tx = self.tx.clone();

        let stream = device
            .build_input_stream(
                &use_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Convert f32 samples to raw little-endian bytes.
                    let bytes: Vec<u8> = data.iter().flat_map(|s| s.to_le_bytes()).collect();
                    // If the channel is full (consumer is slow), drop this
                    // chunk — stale audio data is useless for real-time use.
                    if tx.try_send(bytes).is_err() {
                        // channel full, intentionally dropped
                    }
                },
                |err| {
                    tracing::error!("Audio capture stream error: {err}");
                },
                None,
            )
            .map_err(|e| Error::Audio(format!("Failed to build input stream: {e}")))?;

        stream
            .play()
            .map_err(|e| Error::Audio(format!("Failed to start input stream: {e}")))?;

        self.sample_rate = use_config.sample_rate;
        self.stream = Some(stream);
        tracing::info!(
            "Audio capture started: {} Hz, {} ch, f32",
            use_config.sample_rate,
            use_config.channels
        );
        Ok(())
    }

    /// Stop the capture stream.
    pub fn stop(&mut self) -> Result<()> {
        if let Some(stream) = self.stream.take() {
            drop(stream);
            self.sample_rate = 0;
            tracing::info!("Audio capture stopped");
        }
        Ok(())
    }

    /// Retrieve the next captured audio chunk, if available.
    ///
    /// Each chunk is a `Vec<u8>` containing raw little-endian f32 samples.
    /// Returns `None` if no chunk is currently available.
    pub fn recv_chunk(&self) -> Option<Vec<u8>> {
        self.rx.lock().ok().and_then(|guard| guard.try_recv().ok())
    }
}

impl Drop for AudioCapturer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capturer_new() {
        let capturer = AudioCapturer::new();
        assert!(capturer.is_ok());
        let capturer = capturer.unwrap();
        assert!(capturer.stream.is_none());
        assert!(capturer.recv_chunk().is_none());
    }

    #[test]
    fn test_capturer_stop_when_not_started() {
        let mut capturer = AudioCapturer::new().unwrap();
        let result = capturer.stop();
        assert!(result.is_ok());
    }

    #[test]
    fn test_capturer_recv_empty() {
        let capturer = AudioCapturer::new().unwrap();
        assert!(capturer.recv_chunk().is_none());
    }
}
