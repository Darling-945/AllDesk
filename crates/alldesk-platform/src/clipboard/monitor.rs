use alldesk_core::{Error, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Clipboard content types supported for sync between peers.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardContent {
    Text(String),
    /// Raw BGRA pixel data (Blue, Green, Red, Alpha byte order; width * height * 4 bytes).
    Image {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    },
}

impl ClipboardContent {
    /// Compute a hash of the content for change detection.
    pub(crate) fn content_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        match self {
            ClipboardContent::Text(s) => {
                0u8.hash(&mut hasher);
                s.hash(&mut hasher);
            }
            ClipboardContent::Image {
                width,
                height,
                pixels,
            } => {
                1u8.hash(&mut hasher);
                width.hash(&mut hasher);
                height.hash(&mut hasher);
                pixels.hash(&mut hasher);
            }
        }
        hasher.finish()
    }
}

/// Monitors the system clipboard for content changes.
///
/// Polls the clipboard via arboard and detects changes by hashing the
/// current content and comparing against the previously seen hash.
pub struct ClipboardMonitor {
    clipboard: arboard::Clipboard,
    last_hash: u64,
    last_content: Option<ClipboardContent>,
}

impl ClipboardMonitor {
    /// Create a new clipboard monitor.
    ///
    /// Reads the current clipboard state so that `has_changed()` only
    /// returns `true` for subsequent changes.
    pub fn new() -> Result<Self> {
        let clipboard = arboard::Clipboard::new()
            .map_err(|e| Error::Clipboard(format!("Failed to open clipboard: {e}")))?;

        let mut monitor = Self {
            clipboard,
            last_hash: 0,
            last_content: None,
        };

        // Snapshot the current clipboard so we don't fire a false-positive
        // change on the first `has_changed()` call.
        if let Ok(content) = monitor.read_clipboard() {
            monitor.last_hash = content.content_hash();
            monitor.last_content = Some(content);
        }

        Ok(monitor)
    }

    /// Returns `true` if the clipboard content has changed since the last call.
    pub fn has_changed(&mut self) -> bool {
        match self.read_clipboard() {
            Ok(content) => {
                let hash = content.content_hash();
                if hash != self.last_hash {
                    self.last_hash = hash;
                    self.last_content = Some(content);
                    true
                } else {
                    false
                }
            }
            // Clipboard might be empty or hold an unsupported format;
            // treat this as "no change" rather than an error.
            Err(_) => false,
        }
    }

    /// Return the current clipboard content.
    ///
    /// Tries text first, then image. Returns `None` if neither is available.
    pub fn get_content(&mut self) -> Result<Option<ClipboardContent>> {
        match self.read_clipboard() {
            Ok(content) => {
                self.last_hash = content.content_hash();
                self.last_content = Some(content.clone());
                Ok(Some(content))
            }
            Err(Error::Clipboard(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Write content to the system clipboard.
    pub fn set_content(&mut self, content: &ClipboardContent) -> Result<()> {
        match content {
            ClipboardContent::Text(text) => {
                self.clipboard
                    .set_text(text)
                    .map_err(|e| Error::Clipboard(format!("Failed to set text: {e}")))?;
            }
            ClipboardContent::Image {
                width,
                height,
                pixels,
            } => {
                let image_data = arboard::ImageData {
                    width: *width,
                    height: *height,
                    bytes: std::borrow::Cow::Borrowed(pixels.as_slice()),
                };
                self.clipboard
                    .set_image(image_data)
                    .map_err(|e| Error::Clipboard(format!("Failed to set image: {e}")))?;
            }
        }
        // Update cached state so has_changed() won't immediately fire.
        self.last_hash = content.content_hash();
        self.last_content = Some(content.clone());
        Ok(())
    }

    /// Internal helper: read whatever is on the clipboard right now.
    fn read_clipboard(&mut self) -> Result<ClipboardContent> {
        // Try text first (most common case).
        if let Ok(text) = self.clipboard.get_text() {
            return Ok(ClipboardContent::Text(text));
        }

        // Fall back to image.
        if let Ok(image) = self.clipboard.get_image() {
            return Ok(ClipboardContent::Image {
                width: image.width,
                height: image.height,
                pixels: image.bytes.into_owned(),
            });
        }

        Err(Error::Clipboard(
            "Clipboard is empty or holds unsupported content".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_content_hash_deterministic() {
        let a = ClipboardContent::Text("hello".into());
        let b = ClipboardContent::Text("hello".into());
        assert_eq!(a.content_hash(), b.content_hash());

        let c = ClipboardContent::Text("world".into());
        assert_ne!(a.content_hash(), c.content_hash());
    }

    #[test]
    fn image_content_hash_deterministic() {
        let a = ClipboardContent::Image {
            width: 2,
            height: 1,
            pixels: vec![255, 0, 0, 255, 0, 255, 0, 255],
        };
        let b = ClipboardContent::Image {
            width: 2,
            height: 1,
            pixels: vec![255, 0, 0, 255, 0, 255, 0, 255],
        };
        assert_eq!(a.content_hash(), b.content_hash());
    }
}
