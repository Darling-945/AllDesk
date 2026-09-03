#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Video,
    Audio,
    Input,
    Clipboard,
    File,
    Whiteboard,
    Control,
}

impl Channel {
    pub fn stream_id(self) -> u64 {
        match self {
            Self::Video => 0,
            Self::Audio => 1,
            Self::Input => 2,
            Self::Clipboard => 3,
            Self::File => 4,
            Self::Whiteboard => 5,
            Self::Control => 6,
        }
    }

    pub fn is_datagram(self) -> bool {
        matches!(self, Self::Audio)
    }

    /// Reconstruct a Channel from the stream_id value produced by `stream_id()`.
    pub fn from_stream_id(id: u64) -> Option<Self> {
        match id {
            0 => Some(Self::Video),
            1 => Some(Self::Audio),
            2 => Some(Self::Input),
            3 => Some(Self::Clipboard),
            4 => Some(Self::File),
            5 => Some(Self::Whiteboard),
            6 => Some(Self::Control),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_stream_id_roundtrip() {
        for ch in [
            Channel::Video,
            Channel::Audio,
            Channel::Input,
            Channel::Clipboard,
            Channel::File,
            Channel::Whiteboard,
            Channel::Control,
        ] {
            assert_eq!(Channel::from_stream_id(ch.stream_id()), Some(ch));
        }
    }

    #[test]
    fn test_channel_from_invalid_stream_id() {
        assert_eq!(Channel::from_stream_id(7), None);
        assert_eq!(Channel::from_stream_id(255), None);
    }

    #[test]
    fn test_channel_is_datagram() {
        assert!(Channel::Audio.is_datagram());
        assert!(!Channel::Video.is_datagram());
        assert!(!Channel::Input.is_datagram());
        assert!(!Channel::Clipboard.is_datagram());
        assert!(!Channel::File.is_datagram());
        assert!(!Channel::Whiteboard.is_datagram());
        assert!(!Channel::Control.is_datagram());
    }

    #[test]
    fn test_channel_stream_ids_unique() {
        let ids: Vec<u64> = [
            Channel::Video,
            Channel::Audio,
            Channel::Input,
            Channel::Clipboard,
            Channel::File,
            Channel::Whiteboard,
            Channel::Control,
        ]
        .iter()
        .map(|c| c.stream_id())
        .collect();
        let unique: std::collections::HashSet<u64> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len());
    }
}
