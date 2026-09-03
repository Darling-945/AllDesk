use alldesk_capture::capture::CapturedFrame;
use alldesk_core::Result;

// Adaptive bitrate/framerate policies live in alldesk-core so the control
// loop can be shared with the transport layer without depending on a codec.
pub use alldesk_core::adaptive::{AdaptiveBitrate, AdaptiveFramerate};

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
