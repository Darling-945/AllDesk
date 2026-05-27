use alldesk_core::{Error, Result};
use crate::ClipboardContent;

/// Wire-format tag for the content type.
const TAG_TEXT: u8 = 0x01;
const TAG_IMAGE: u8 = 0x02;

/// Manages clipboard synchronisation between remote peers.
///
/// The serialisation format is a simple length-prefixed binary encoding
/// that avoids pulling in a full framework like bincode:
///
/// ```text
/// [1 byte: tag] [4 bytes: payload_len (big-endian)] [payload_len bytes: payload]
/// ```
///
/// For images the payload layout is:
/// ```text
/// [4 bytes: width (LE)] [4 bytes: height (LE)] [remaining: RGBA pixels]
/// ```
pub struct ClipboardSync {
    /// Set to true when the local clipboard was changed by us (receive path)
    /// so that the monitor can suppress sending it back to the remote peer.
    last_remote_update_hash: u64,
}

impl ClipboardSync {
    pub fn new() -> Result<Self> {
        Ok(Self {
            last_remote_update_hash: 0,
        })
    }

    /// Serialize clipboard content into bytes ready to send over the network.
    pub fn serialize_content(content: &ClipboardContent) -> Vec<u8> {
        match content {
            ClipboardContent::Text(text) => {
                let text_bytes = text.as_bytes();
                let mut buf = Vec::with_capacity(1 + 4 + text_bytes.len());
                buf.push(TAG_TEXT);
                buf.extend_from_slice(&(text_bytes.len() as u32).to_be_bytes());
                buf.extend_from_slice(text_bytes);
                buf
            }
            ClipboardContent::Image { width, height, pixels } => {
                let mut buf = Vec::with_capacity(1 + 4 + 8 + pixels.len());
                buf.push(TAG_IMAGE);
                // payload = width(4) + height(4) + pixels
                let payload_len = 8 + pixels.len();
                buf.extend_from_slice(&(payload_len as u32).to_be_bytes());
                buf.extend_from_slice(&(*width as u32).to_le_bytes());
                buf.extend_from_slice(&(*height as u32).to_le_bytes());
                buf.extend_from_slice(pixels);
                buf
            }
        }
    }

    /// Deserialize bytes received from the network into `ClipboardContent`.
    pub fn deserialize_content(data: &[u8]) -> Result<ClipboardContent> {
        if data.is_empty() {
            return Err(Error::Clipboard("Empty clipboard data".into()));
        }

        let tag = data[0];
        let rest = &data[1..];

        if rest.len() < 4 {
            return Err(Error::Clipboard("Payload length missing".into()));
        }

        let payload_len = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let payload = rest.get(4..4 + payload_len).ok_or_else(|| {
            Error::Clipboard(format!(
                "Payload too short: expected {payload_len}, got {}",
                rest.len().saturating_sub(4)
            ))
        })?;

        match tag {
            TAG_TEXT => {
                let text = String::from_utf8(payload.to_vec())
                    .map_err(|e| Error::Clipboard(format!("Invalid UTF-8 in text: {e}")))?;
                Ok(ClipboardContent::Text(text))
            }
            TAG_IMAGE => {
                if payload.len() < 8 {
                    return Err(Error::Clipboard("Image payload too short for dimensions".into()));
                }
                let width = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
                let height = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
                let pixels = payload[8..].to_vec();

                let expected = width.checked_mul(height)
                    .and_then(|sz| sz.checked_mul(4))
                    .ok_or_else(|| Error::Clipboard("Image dimensions overflow".into()))?;

                if pixels.len() != expected {
                    return Err(Error::Clipboard(format!(
                        "Pixel data size mismatch: expected {expected} bytes ({}x{} RGBA), got {}",
                        width, height, pixels.len()
                    )));
                }

                Ok(ClipboardContent::Image { width, height, pixels })
            }
            _ => Err(Error::Clipboard(format!("Unknown content tag: {tag}"))),
        }
    }

    /// Serialize the given content and return bytes suitable for sending.
    /// This is a convenience wrapper around `serialize_content`.
    pub fn send_clipboard(content: &ClipboardContent) -> Vec<u8> {
        Self::serialize_content(content)
    }

    /// Receive clipboard data from a remote peer, deserialize it, and
    /// write it to the local system clipboard.
    pub async fn receive_clipboard(
        &self,
        monitor: &mut crate::ClipboardMonitor,
        data: &[u8],
    ) -> Result<()> {
        let content = Self::deserialize_content(data)?;

        // Record the hash so the caller can suppress a round-trip echo.
        // (The monitor's set_content also updates its internal hash.)

        monitor.set_content(&content)?;

        tracing::debug!(
            "Applied remote clipboard update: {}",
            match &content {
                ClipboardContent::Text(t) => format!("text ({} bytes)", t.len()),
                ClipboardContent::Image { width, height, .. } =>
                    format!("image ({}x{})", width, height),
            }
        );

        Ok(())
    }

    /// Check whether a locally-detected clipboard change is just an echo
    /// of content we ourselves pushed in `receive_clipboard`.
    pub fn is_remote_update(&self, content: &ClipboardContent) -> bool {
        content.content_hash() == self.last_remote_update_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_text() {
        let original = ClipboardContent::Text("Hello, remote peer!".into());
        let bytes = ClipboardSync::serialize_content(&original);
        let decoded = ClipboardSync::deserialize_content(&bytes).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn roundtrip_image() {
        let original = ClipboardContent::Image {
            width: 3,
            height: 2,
            pixels: vec![255u8; 3 * 2 * 4], // all-white 3x2 RGBA
        };
        let bytes = ClipboardSync::serialize_content(&original);
        let decoded = ClipboardSync::deserialize_content(&bytes).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn deserialize_empty_is_error() {
        let result = ClipboardSync::deserialize_content(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_bad_tag_is_error() {
        // tag=0x99, then a valid-looking payload length
        let data: Vec<u8> = vec![0x99, 0, 0, 0, 1, 0x00];
        let result = ClipboardSync::deserialize_content(&data);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_truncated_payload_is_error() {
        let data: Vec<u8> = vec![TAG_TEXT, 0, 0, 0, 10, b'h', b'i']; // claims 10 bytes, has 2
        let result = ClipboardSync::deserialize_content(&data);
        assert!(result.is_err());
    }
}
