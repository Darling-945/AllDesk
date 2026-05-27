use alldesk_capture::capture::{CaptureConfig, CaptureProvider, CapturedFrame, FrameData};
use alldesk_core::Result;
use alldesk_net::channel::Channel;
use alldesk_net::transport::QuicTransport;
use alldesk_net::Transport;
use tokio::sync::broadcast;

#[cfg(target_os = "windows")]
use alldesk_capture::dxgi::DxgiCapturer;

#[cfg(target_os = "android")]
use alldesk_capture::android::AndroidCapturer;

use alldesk_codec::encoder::{Codec, EncodedPacket, VideoEncoder};
use alldesk_codec::decoder::VideoDecoder;
use alldesk_codec::vp9::{Vp9Encoder, Vp9Decoder};

const FRAME_TYPE_RAW: u8 = 0x00;
const FRAME_TYPE_VP9: u8 = 0x01;

/// A decoded video frame ready for display.
#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub bgra_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Orchestrates capture → VP9 encode → QUIC send.
pub struct SenderPipeline {
    capturer: Box<dyn CaptureProvider>,
    transport: QuicTransport,
    fps: u32,
    encoder: Option<Vp9Encoder>,
}

unsafe impl Send for SenderPipeline {}

impl SenderPipeline {
    pub async fn new(transport: QuicTransport, bitrate_kbps: u32, fps: u32) -> Result<Self> {
        #[cfg(target_os = "windows")]
        let mut capturer: Box<dyn CaptureProvider> = Box::new(DxgiCapturer::new());

        #[cfg(target_os = "android")]
        let mut capturer: Box<dyn CaptureProvider> = Box::new(AndroidCapturer::new());

        #[cfg(not(any(target_os = "windows", target_os = "android")))]
        let mut capturer: Box<dyn CaptureProvider> = {
            return Err(alldesk_core::Error::Capture("no capture backend".into()));
        };

        let monitors = capturer.enumerate_monitors().await?;
        if monitors.is_empty() {
            return Err(alldesk_core::Error::Capture("no monitors found".into()));
        }
        let mon = &monitors[0];

        capturer.start_capture(CaptureConfig {
            monitor_id: mon.id,
            fps,
            show_cursor: true,
        }).await?;

        let encoder = match Vp9Encoder::new(mon.width, mon.height, bitrate_kbps, fps) {
            Ok(enc) => {
                tracing::info!("VP9 encoder initialized ({}x{} @ {}kbps)", mon.width, mon.height, bitrate_kbps);
                Some(enc)
            }
            Err(e) => {
                tracing::warn!("VP9 encoder init failed ({}), using raw frames", e);
                None
            }
        };

        Ok(Self { capturer, transport, fps, encoder })
    }

    pub async fn run(&mut self) -> Result<()> {
        let interval = std::time::Duration::from_millis(1000 / self.fps as u64);
        let mut tick = tokio::time::interval(interval);
        let mut consecutive_errors = 0u32;

        loop {
            tick.tick().await;

            match self.capturer.next_frame().await {
                Ok(Some(frame)) => {
                    consecutive_errors = 0;

                    if let Some(ref mut encoder) = self.encoder {
                        match encoder.encode(&frame) {
                            Ok(packets) => {
                                for pkt in packets {
                                    if let Err(e) = self.send_vpx_frame(&frame, &pkt).await {
                                        tracing::warn!("send vp9: {}", e);
                                    }
                                }
                                continue;
                            }
                            Err(e) => {
                                tracing::warn!("VP9 encode error, sending raw: {}", e);
                            }
                        }
                    }
                    let _ = self.send_raw_frame(&frame).await;
                }
                Ok(None) => {}
                Err(e) => {
                    consecutive_errors += 1;
                    if consecutive_errors > 60 {
                        return Err(e);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }
    }

    async fn send_vpx_frame(&mut self, frame: &CapturedFrame, pkt: &EncodedPacket) -> Result<()> {
        let mut out = Vec::with_capacity(9 + pkt.data.len());
        out.extend_from_slice(&frame.width.to_le_bytes());
        out.extend_from_slice(&frame.height.to_le_bytes());
        out.push(FRAME_TYPE_VP9);
        out.extend_from_slice(&pkt.data);
        self.transport.send(Channel::Video, &out).await
    }

    async fn send_raw_frame(&mut self, frame: &CapturedFrame) -> Result<()> {
        let bgra = match &frame.data {
            FrameData::Cpu(data) => data.clone(),
            _ => return Ok(()),
        };

        let mut out = Vec::with_capacity(9 + bgra.len());
        out.extend_from_slice(&frame.width.to_le_bytes());
        out.extend_from_slice(&frame.height.to_le_bytes());
        out.push(FRAME_TYPE_RAW);
        out.extend_from_slice(&bgra);

        if let Err(e) = self.transport.send(Channel::Video, &out).await {
            tracing::warn!("send raw frame: {}", e);
        }
        Ok(())
    }
}

/// Orchestrates QUIC recv → VP9 decode → push BGRA frames to listeners.
pub struct ReceiverPipeline {
    transport: QuicTransport,
    frame_tx: broadcast::Sender<VideoFrame>,
    decoder: Option<Vp9Decoder>,
}

impl ReceiverPipeline {
    pub fn new(transport: QuicTransport, _width: u32, _height: u32) -> (Self, broadcast::Receiver<VideoFrame>) {
        let (tx, rx) = broadcast::channel(8);
        let this = Self { transport, frame_tx: tx, decoder: None };
        (this, rx)
    }

    pub async fn run(&mut self) -> Result<()> {
        let ch = self.transport.accept_stream().await?;
        if ch != Channel::Video {
            return Err(alldesk_core::Error::Network(
                format!("expected Video channel, got {:?}", ch)
            ));
        }

        loop {
            match self.transport.recv(Channel::Video).await {
                Ok(data) if data.len() > 9 => {
                    let width = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4]));
                    let height = u32::from_le_bytes(data[4..8].try_into().unwrap_or([0; 4]));
                    let frame_type = data[8];
                    let payload = &data[9..];

                    if width == 0 || height == 0 {
                        continue;
                    }

                    let bgra_data = match frame_type {
                        FRAME_TYPE_VP9 => self.decode_vp9(payload, width, height),
                        _ => payload.to_vec(),
                    };

                    if bgra_data.len() == width as usize * height as usize * 4 {
                        let vf = VideoFrame { bgra_data, width, height };
                        let _ = self.frame_tx.send(vf);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("recv video: {}", e);
                    tokio::time::sleep(std::time::Duration::from_millis(16)).await;
                }
            }
        }
    }

    fn decode_vp9(&mut self, payload: &[u8], width: u32, height: u32) -> Vec<u8> {
        if self.decoder.is_none() {
            match Vp9Decoder::new(width, height) {
                Ok(dec) => {
                    tracing::info!("VP9 decoder initialized ({}x{})", width, height);
                    self.decoder = Some(dec);
                }
                Err(e) => {
                    tracing::warn!("VP9 decoder init failed: {}", e);
                    return Vec::new();
                }
            }
        }

        let packet = EncodedPacket {
            data: payload.to_vec(),
            is_keyframe: false,
            timestamp_ms: 0,
            codec: Codec::VP9,
        };

        match self.decoder.as_mut().unwrap().decode(&packet) {
            Ok(decoded) => decoded.data,
            Err(e) => {
                tracing::debug!("VP9 decode: {}", e);
                Vec::new()
            }
        }
    }
}
