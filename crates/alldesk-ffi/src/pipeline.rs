use alldesk_capture::capture::{CaptureConfig, CaptureProvider, CapturedFrame, FrameData};
use alldesk_core::Result;
use alldesk_net::channel::Channel;
use alldesk_net::flow::{FlowConfig, FlowController};
use alldesk_net::transport::QuicTransport;
use alldesk_net::Transport;
use alldesk_platform::audio::{AudioCapturer, AudioPlayer};
use alldesk_platform::clipboard::{ClipboardMonitor, ClipboardSync};
use alldesk_recording::Recorder;
use tokio::sync::broadcast;

#[cfg(target_os = "windows")]
use alldesk_capture::dxgi::DxgiCapturer;

#[cfg(target_os = "android")]
use alldesk_capture::android::AndroidCapturer;

use alldesk_codec::decoder::VideoDecoder;
use alldesk_codec::encoder::{Codec, EncodedPacket, VideoEncoder};
use alldesk_codec::vp9::{Vp9Decoder, Vp9Encoder};

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
    /// Current encoder bitrate, tracked so adaptive updates that don't change
    /// it skip the libvpx reconfiguration.
    bitrate_kbps: u32,
    encoder: Option<Vp9Encoder>,
    /// Bounded send queue: when the network can't keep up, stale frames are
    /// dropped instead of unbounded buffering.
    flow: FlowController,
    /// New encoder/pacing targets from the adaptive controller task, if one
    /// is attached to this session.
    adaptive: Option<tokio::sync::watch::Receiver<alldesk_core::adaptive::AdaptiveTargets>>,
}

unsafe impl Send for SenderPipeline {}

/// Give up on the session after this many consecutive transport failures
/// (≈2 s at 30 fps). Without it the capture/encode loop would keep running —
/// and logging a warning per frame — long after the viewer disconnected.
const MAX_CONSECUTIVE_SEND_ERRORS: u32 = 60;

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

        capturer
            .start_capture(CaptureConfig {
                monitor_id: mon.id,
                fps,
                show_cursor: true,
            })
            .await?;

        let encoder = match Vp9Encoder::new(mon.width, mon.height, bitrate_kbps, fps) {
            Ok(enc) => {
                tracing::info!(
                    "VP9 encoder initialized ({}x{} @ {}kbps)",
                    mon.width,
                    mon.height,
                    bitrate_kbps
                );
                Some(enc)
            }
            Err(e) => {
                tracing::warn!("VP9 encoder init failed ({}), using raw frames", e);
                None
            }
        };

        Ok(Self {
            capturer,
            transport,
            fps,
            bitrate_kbps,
            encoder,
            flow: Self::build_flow(mon.width, mon.height),
            adaptive: None,
        })
    }

    /// Subscribe the pipeline to adaptive encoder/pacing targets.
    pub fn with_adaptive(
        mut self,
        targets: tokio::sync::watch::Receiver<alldesk_core::adaptive::AdaptiveTargets>,
    ) -> Self {
        self.adaptive = Some(targets);
        self
    }

    /// Apply new adaptive targets to the encoder and the capture pacing.
    /// Reconfigures only when a value actually changed. Pacing picks the new
    /// FPS up on the next `run()` tick (the scheduler re-reads `self.fps`).
    fn apply_adaptive_targets(&mut self, targets: alldesk_core::adaptive::AdaptiveTargets) {
        let new_bitrate = targets.bitrate_kbps;
        // The controller guarantees fps ≥ 1; clamp defensively anyway since
        // pacing divides by it.
        let new_fps = targets.fps.max(1);
        let bitrate_changed = new_bitrate != self.bitrate_kbps;
        let fps_changed = new_fps != self.fps;
        if !bitrate_changed && !fps_changed {
            return;
        }

        if let Some(encoder) = self.encoder.as_mut() {
            encoder.reconfigure(new_bitrate, new_fps);
        }
        self.bitrate_kbps = new_bitrate;
        self.fps = new_fps;
        tracing::info!(
            "adaptive targets applied: {} kbps @ {} fps",
            new_bitrate,
            new_fps
        );
    }

    /// Flow control sized for this stream: messages up to one raw frame,
    /// a small queue, and a short TTL so only recent frames are sent.
    fn build_flow(width: u32, height: u32) -> FlowController {
        FlowController::with_config(FlowConfig {
            send_buffer_capacity: 4,
            recv_buffer_capacity: 4,
            max_message_size: width as usize * height as usize * 4 + 4096,
            message_ttl: std::time::Duration::from_millis(500),
        })
    }

    /// Enqueue one video message, dropping it (or stale queued frames) when
    /// the network is backpressured, then drain everything that survived.
    /// Returns the first transport error so the caller can count failures.
    async fn send_video_message(&mut self, data: Vec<u8>) -> Result<()> {
        if !self.flow.try_send(Channel::Video, data) {
            // Queue full: drop this frame — real-time video prefers fresh.
            // Backpressure is not a connection error, so report success.
            return Ok(());
        }
        while let Some((channel, data)) = self.flow.poll_send() {
            if let Err(e) = self.transport.send(channel, &data).await {
                tracing::warn!("send video: {}", e);
                return Err(e);
            }
        }
        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut consecutive_errors = 0u32;
        let mut consecutive_send_errors = 0u32;
        // Fixed-rate scheduler over sleep_until instead of tokio::time::interval:
        // the period must follow the adaptive FPS, and Interval's period
        // can't be changed after creation.
        let mut next_tick = tokio::time::Instant::now();

        loop {
            let period = std::time::Duration::from_millis(1000 / self.fps.max(1) as u64);
            next_tick += period;
            // Behind schedule (slow capture/encode): skip missed ticks
            // rather than bursting — real-time video prefers fresh frames.
            let now = tokio::time::Instant::now();
            if next_tick <= now {
                next_tick = now + period;
            }
            tokio::time::sleep_until(next_tick).await;

            // Apply the newest adaptive target, if any (coalesce a burst of
            // updates into the latest one).
            let newest_target = self.adaptive.as_mut().and_then(|rx| {
                let mut latest = None;
                while rx.has_changed().unwrap_or(false) {
                    latest = Some(*rx.borrow_and_update());
                }
                latest
            });
            if let Some(targets) = newest_target {
                self.apply_adaptive_targets(targets);
            }

            match self.capturer.next_frame().await {
                Ok(Some(frame)) => {
                    consecutive_errors = 0;

                    if let Some(ref mut encoder) = self.encoder {
                        match encoder.encode(&frame) {
                            Ok(packets) => {
                                for pkt in packets {
                                    if let Err(e) = self.send_vpx_frame(&frame, &pkt).await {
                                        tracing::warn!("send vp9: {}", e);
                                        consecutive_send_errors += 1;
                                        if consecutive_send_errors > MAX_CONSECUTIVE_SEND_ERRORS {
                                            return Err(e);
                                        }
                                    } else {
                                        consecutive_send_errors = 0;
                                    }
                                }
                                continue;
                            }
                            Err(e) => {
                                tracing::warn!("VP9 encode error, sending raw: {}", e);
                            }
                        }
                    }
                    match self.send_raw_frame(&frame).await {
                        Ok(()) => consecutive_send_errors = 0,
                        Err(e) => {
                            tracing::warn!("send raw frame: {}", e);
                            consecutive_send_errors += 1;
                            if consecutive_send_errors > MAX_CONSECUTIVE_SEND_ERRORS {
                                return Err(e);
                            }
                        }
                    }
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
        self.send_video_message(out).await
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

        self.send_video_message(out).await
    }
}

/// Maximum time to wait for the sender to open the Video stream.
const ACCEPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Orchestrates QUIC recv → VP9 decode → push BGRA frames to listeners.
pub struct ReceiverPipeline {
    transport: QuicTransport,
    frame_tx: broadcast::Sender<VideoFrame>,
    decoder: Option<Vp9Decoder>,
    /// Active recorder and when its recording started. Pulled fresh from the
    /// global slot every frame so recording can start/stop mid-session.
    recorder: Option<(
        std::sync::Arc<std::sync::Mutex<Recorder>>,
        std::time::Instant,
    )>,
}

impl ReceiverPipeline {
    pub fn new(
        transport: QuicTransport,
        _width: u32,
        _height: u32,
    ) -> (Self, broadcast::Receiver<VideoFrame>) {
        let (tx, rx) = broadcast::channel(8);
        let this = Self {
            transport,
            frame_tx: tx,
            decoder: None,
            recorder: None,
        };
        (this, rx)
    }

    /// Append an encoded frame to the session recorder, if one is active.
    /// Only VP9 frames are stored (raw BGRA would be ≈250 MB/s at 1080p30).
    fn record_encoded_frame(&mut self, width: u32, height: u32, payload: &[u8]) {
        let Some(rec) = crate::api::current_recorder() else {
            return;
        };
        // New recorder instance → restart the timestamp base.
        if self
            .recorder
            .as_ref()
            .is_none_or(|(r, _)| !std::sync::Arc::ptr_eq(r, &rec))
        {
            self.recorder = Some((rec, std::time::Instant::now()));
        }
        let Some((rec, start)) = self.recorder.as_ref() else {
            return;
        };
        let ts = start.elapsed().as_millis() as u64;
        if let Ok(mut r) = rec.lock() {
            // First frame (or a resolution change) fixes the header dims.
            if r.width() != width || r.height() != height {
                r.set_dimensions(width, height);
                r.set_fps(30);
            }
            if let Err(e) = r.write_frame(payload, ts) {
                tracing::warn!("record frame: {}", e);
            }
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // Other pipelines (input, clipboard) share the connection; accept only
        // the Video stream and leave the rest to their own accept_channel calls.
        tokio::time::timeout(
            ACCEPT_TIMEOUT,
            self.transport.accept_channel(Channel::Video),
        )
        .await
        .map_err(|_| {
            alldesk_core::Error::Network(
                "timed out waiting for sender to open Video stream (15s)".into(),
            )
        })?
        .map_err(|e| alldesk_core::Error::Network(format!("accept Video stream: {}", e)))?;

        tracing::info!("Video stream accepted, starting recv loop");
        let mut consecutive_errors = 0u32;

        loop {
            match self.transport.recv(Channel::Video).await {
                Ok(data) if data.len() > 9 => {
                    consecutive_errors = 0;
                    let width = u32::from_le_bytes(data[0..4].try_into().unwrap_or([0; 4]));
                    let height = u32::from_le_bytes(data[4..8].try_into().unwrap_or([0; 4]));
                    let frame_type = data[8];
                    let payload = &data[9..];

                    if width == 0 || height == 0 {
                        continue;
                    }

                    if frame_type == FRAME_TYPE_VP9 {
                        self.record_encoded_frame(width, height, payload);
                    }

                    let bgra_data = match frame_type {
                        FRAME_TYPE_VP9 => self.decode_vp9(payload, width, height),
                        _ => payload.to_vec(),
                    };

                    if bgra_data.len() == width as usize * height as usize * 4 {
                        let vf = VideoFrame {
                            bgra_data,
                            width,
                            height,
                        };
                        let _ = self.frame_tx.send(vf);
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    consecutive_errors += 1;
                    if consecutive_errors > 60 {
                        tracing::warn!("recv video failed repeatedly: {}", e);
                        return Err(e);
                    }
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

// ---------------------------------------------------------------------------
// Audio pipelines
// ---------------------------------------------------------------------------

/// Magic prefix of the stream-format header the audio sender transmits before
/// any PCM data: [magic: 4 bytes] [sample_rate: u32 LE]. Chosen so random PCM
/// data is vanishingly unlikely to collide with it.
const AUDIO_HEADER_MAGIC: [u8; 4] = [0xA1, 0xD1, 0x0D, 0x5A];

/// Largest PCM piece sent as one QUIC datagram (must stay below the ~1200
/// byte path MTU). Multiple of 4 so every piece holds whole f32 samples.
const AUDIO_MAX_PIECE: usize = 1152;

/// Host side: captures audio from microphone → sends over QUIC Audio channel.
pub struct AudioSenderPipeline {
    capturer: AudioCapturer,
    transport: QuicTransport,
}

unsafe impl Send for AudioSenderPipeline {}

impl AudioSenderPipeline {
    pub fn new(transport: QuicTransport) -> Result<Self> {
        let capturer = AudioCapturer::new()?;
        Ok(Self {
            capturer,
            transport,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        self.capturer.start()?;
        let rate = self.capturer.sample_rate();
        tracing::info!("audio sender pipeline started ({} Hz)", rate);

        // Tell the receiver which rate we capture at so it can play at the
        // right speed (the capture device may not support 48 kHz).
        let mut header = AUDIO_HEADER_MAGIC.to_vec();
        header.extend_from_slice(&rate.to_le_bytes());
        self.transport.send(Channel::Audio, &header).await?;

        loop {
            match self.capturer.recv_chunk() {
                Some(chunk) => {
                    // Split into datagram-sized pieces; raw PCM has no framing,
                    // so the receiver can play the pieces back-to-back.
                    // Send errors (connection closed) are fatal for this loop.
                    for piece in chunk.chunks(AUDIO_MAX_PIECE) {
                        self.transport.send(Channel::Audio, piece).await?;
                    }
                }
                None => {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            }
        }
    }
}

/// Viewer side: receives audio from QUIC Audio channel → plays to speaker.
pub struct AudioReceiverPipeline {
    transport: QuicTransport,
}

unsafe impl Send for AudioReceiverPipeline {}

impl AudioReceiverPipeline {
    pub fn new(transport: QuicTransport) -> Self {
        Self { transport }
    }

    pub async fn run(&mut self) -> Result<()> {
        tracing::info!("audio receiver pipeline started");
        let mut player: Option<AudioPlayer> = None;

        loop {
            match self.transport.recv(Channel::Audio).await {
                Ok(data) if data.len() == 8 && data[..4] == AUDIO_HEADER_MAGIC => {
                    let fallback_rate = 48_000u32.to_le_bytes();
                    let rate = u32::from_le_bytes(data[4..8].try_into().unwrap_or(fallback_rate));
                    tracing::info!("audio receiver: sender sample rate {} Hz", rate);
                    match AudioPlayer::with_sample_rate(rate) {
                        Ok(p) => player = Some(p),
                        Err(e) => tracing::warn!("audio player init ({} Hz): {}", rate, e),
                    }
                }
                Ok(data) => {
                    if data.is_empty() || data.len() % 4 != 0 {
                        continue;
                    }
                    if player.is_none() {
                        // Sender did not announce its rate — assume the default.
                        match AudioPlayer::new() {
                            Ok(p) => player = Some(p),
                            Err(e) => {
                                tracing::warn!("audio player init: {}", e);
                                continue;
                            }
                        }
                    }
                    if let Err(e) = player.as_ref().unwrap().play(&data) {
                        tracing::debug!("audio play: {}", e);
                    }
                }
                Err(e) => {
                    // Transport errors (connection closed) are fatal here; without
                    // this the loop would spin forever after disconnect.
                    tracing::debug!("recv audio: {}", e);
                    return Err(e);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Clipboard sync pipeline
// ---------------------------------------------------------------------------

/// Bidirectional clipboard sync: poll the local clipboard for changes and
/// send them to the peer, while a dedicated receive task applies incoming
/// updates. Both host and viewer use the same logic; the initiator opens
/// the stream.
///
/// The receive loop deliberately runs as its own task: awaiting `recv()` in a
/// `tokio::select!` loop is not cancellation-safe (a partial length-prefix
/// read is dropped whenever the timer branch wins), which would permanently
/// desynchronize the length-prefixed stream framing.
pub struct ClipboardPipeline {
    transport: QuicTransport,
    monitor: std::sync::Arc<tokio::sync::Mutex<ClipboardMonitor>>,
    sync: std::sync::Arc<tokio::sync::Mutex<ClipboardSync>>,
    /// If true, this side opens the stream; otherwise it accepts.
    is_initiator: bool,
}

unsafe impl Send for ClipboardPipeline {}

impl ClipboardPipeline {
    pub fn new(transport: QuicTransport, is_initiator: bool) -> Result<Self> {
        let monitor = ClipboardMonitor::new()?;
        let sync = ClipboardSync::new()?;
        Ok(Self {
            transport,
            monitor: std::sync::Arc::new(tokio::sync::Mutex::new(monitor)),
            sync: std::sync::Arc::new(tokio::sync::Mutex::new(sync)),
            is_initiator,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        // Establish the stream
        if self.is_initiator {
            // Send a handshake to open the stream
            self.transport.send(Channel::Clipboard, &[0x00]).await?;
            tracing::info!("clipboard pipeline: stream opened (initiator)");
        } else {
            // Input and Video pipelines share this connection; wait for OUR
            // stream instead of grabbing whatever arrives first.
            tokio::time::timeout(
                ACCEPT_TIMEOUT,
                self.transport.accept_channel(Channel::Clipboard),
            )
            .await
            .map_err(|_| {
                alldesk_core::Error::Network("timed out waiting for Clipboard stream (15s)".into())
            })?
            .map_err(|e| alldesk_core::Error::Network(format!("accept Clipboard: {}", e)))?;
            // Read and discard the handshake byte
            let _ = self.transport.recv(Channel::Clipboard).await;
            tracing::info!("clipboard pipeline: stream accepted");
        }

        // Remote → local clipboard. Runs on a clone of the transport (the
        // stream table is shared), so receive and send use different halves
        // of the stream and never block each other.
        let mut rx_transport = self.transport.clone();
        let monitor = std::sync::Arc::clone(&self.monitor);
        let sync = std::sync::Arc::clone(&self.sync);
        tokio::spawn(async move {
            loop {
                match rx_transport.recv(Channel::Clipboard).await {
                    Ok(data) if data.len() > 1 => {
                        // Lock monitor first, then sync — same order as the
                        // sender loop below — to avoid a lock-order deadlock.
                        let mut mon = monitor.lock().await;
                        let mut sy = sync.lock().await;
                        if let Err(e) = sy.receive_clipboard(&mut mon, &data).await {
                            tracing::debug!("apply remote clipboard: {}", e);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::debug!("recv clipboard: {}", e);
                        break;
                    }
                }
            }
        });

        // Local clipboard → remote, polled.
        let mut poll_interval = tokio::time::interval(std::time::Duration::from_millis(250));
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            poll_interval.tick().await;

            let content = {
                let mut mon = self.monitor.lock().await;
                if !mon.has_changed() {
                    continue;
                }
                match mon.get_content() {
                    Ok(Some(content)) => content,
                    _ => continue,
                }
            };

            if self.sync.lock().await.is_remote_update(&content) {
                continue;
            }

            let data = ClipboardSync::serialize_content(&content);
            self.transport.send(Channel::Clipboard, &data).await?;
        }
    }
}
