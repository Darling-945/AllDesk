use alldesk_core::{Error, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Clipboard content types supported for sync between peers.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipboardContent {
    Text(String),
    Image {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    },
}

impl ClipboardContent {
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

/// Stub clipboard monitor for Android — clipboard is handled via platform channel.
pub struct ClipboardMonitor {
    last_content: Option<ClipboardContent>,
}

impl ClipboardMonitor {
    pub fn new() -> Result<Self> {
        Ok(Self { last_content: None })
    }

    pub fn has_changed(&mut self) -> bool {
        false
    }

    pub fn get_content(&mut self) -> Result<Option<ClipboardContent>> {
        Ok(self.last_content.clone())
    }

    pub fn set_content(&mut self, _content: &ClipboardContent) -> Result<()> {
        Err(Error::Clipboard(
            "Clipboard not supported on Android".into(),
        ))
    }
}
