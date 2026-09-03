use super::encoder::EncodedPacket;
use alldesk_core::Result;

pub struct DecodedFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

pub trait VideoDecoder: Send + Sync {
    fn decode(&mut self, packet: &EncodedPacket) -> Result<DecodedFrame>;
}
