use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub peer_id: String,
    pub peer_name: String,
    pub listen_port: u16,
    pub signaling_server: String,
    pub video: VideoConfig,
    pub audio: AudioConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    pub codec: VideoCodec,
    pub bitrate_kbps: u32,
    pub fps: u32,
    pub monitor_index: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum VideoCodec {
    VP9,
    H264,
    AV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub enabled: bool,
    pub codec: AudioCodec,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AudioCodec {
    Opus,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            peer_id: uuid::Uuid::new_v4().to_string(),
            peer_name: hostname().unwrap_or_else(|_| "Unknown".into()),
            listen_port: 21116,
            signaling_server: "ws://localhost:21118".into(),
            video: VideoConfig {
                codec: VideoCodec::VP9,
                bitrate_kbps: 2000,
                fps: 30,
                monitor_index: 0,
            },
            audio: AudioConfig {
                enabled: true,
                codec: AudioCodec::Opus,
                bitrate_kbps: 32,
            },
        }
    }
}

fn hostname() -> std::result::Result<String, std::io::Error> {
    Ok(std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| {
            #[cfg(unix)]
            {
                std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string())
            }
            #[cfg(windows)]
            {
                std::env::var("USERNAME").map(|u| format!("{}-PC", u))
            }
            #[cfg(not(any(unix, windows)))]
            {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no hostname"))
            }
        })
        .unwrap_or_else(|_| "Unknown".into()))
}
