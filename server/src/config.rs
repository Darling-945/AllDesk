use serde::{Deserialize, Serialize};
use std::path::Path;

/// Server configuration loaded from TOML file, environment variables, or CLI args.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ServerConfig {
    pub signaling_port: u16,
    pub relay_port: u16,
    pub stun_port: u16,
    pub health_port: u16,
    pub log_level: String,
    pub max_message_size: usize,
    pub rate_limit_per_second: u64,
    pub peer_timeout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            signaling_port: 21118,
            relay_port: 21119,
            stun_port: 21116,
            health_port: 21120,
            log_level: "info".to_string(),
            max_message_size: 64 * 1024,
            rate_limit_per_second: 30,
            peer_timeout_secs: 60,
        }
    }
}

impl ServerConfig {
    /// Load configuration from a TOML file.
    #[allow(dead_code)]
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ServerConfig = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Load configuration from environment variables (ALLDESK_ prefix).
    #[allow(dead_code)]
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(v) = std::env::var("ALLDESK_SIGNALING_PORT") {
            if let Ok(port) = v.parse() {
                config.signaling_port = port;
            }
        }
        if let Ok(v) = std::env::var("ALLDESK_RELAY_PORT") {
            if let Ok(port) = v.parse() {
                config.relay_port = port;
            }
        }
        if let Ok(v) = std::env::var("ALLDESK_STUN_PORT") {
            if let Ok(port) = v.parse() {
                config.stun_port = port;
            }
        }
        if let Ok(v) = std::env::var("ALLDESK_HEALTH_PORT") {
            if let Ok(port) = v.parse() {
                config.health_port = port;
            }
        }
        if let Ok(v) = std::env::var("ALLDESK_LOG_LEVEL") {
            config.log_level = v;
        }
        if let Ok(v) = std::env::var("ALLDESK_RATE_LIMIT") {
            if let Ok(limit) = v.parse() {
                config.rate_limit_per_second = limit;
            }
        }

        config
    }

    /// Apply CLI argument overrides to the configuration.
    #[allow(dead_code)]
    pub fn apply_cli_overrides(&mut self, signaling: Option<u16>, relay: Option<u16>, stun: Option<u16>, health: Option<u16>) {
        if let Some(p) = signaling { self.signaling_port = p; }
        if let Some(p) = relay { self.relay_port = p; }
        if let Some(p) = stun { self.stun_port = p; }
        if let Some(p) = health { self.health_port = p; }
    }

    /// Load config from file if it exists, fall back to env then defaults.
    #[allow(dead_code)]
    pub fn load(config_path: Option<&str>) -> Self {
        // Try file first
        if let Some(path) = config_path {
            let path = Path::new(path);
            if path.exists() {
                match Self::from_file(path) {
                    Ok(config) => {
                        tracing::info!("Loaded configuration from {}", path.display());
                        return config;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load config from {}: {}", path.display(), e);
                    }
                }
            }
        }

        // Try default config file locations
        for default_path in &["alldesk-server.toml", "config.toml", "/etc/alldesk/server.toml"] {
            let path = Path::new(default_path);
            if path.exists() {
                if let Ok(config) = Self::from_file(path) {
                    tracing::info!("Loaded configuration from {}", path.display());
                    return config;
                }
            }
        }

        // Fall back to environment variables
        let config = Self::from_env();
        tracing::info!("Using default configuration (with env overrides)");
        config
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.signaling_port == 0 {
            return Err(anyhow::anyhow!("signaling_port cannot be 0"));
        }
        if self.relay_port == 0 {
            return Err(anyhow::anyhow!("relay_port cannot be 0"));
        }
        if self.stun_port == 0 {
            return Err(anyhow::anyhow!("stun_port cannot be 0"));
        }
        if self.rate_limit_per_second == 0 {
            return Err(anyhow::anyhow!("rate_limit_per_second cannot be 0"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_valid() {
        let config = ServerConfig::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.signaling_port, 21118);
        assert_eq!(config.relay_port, 21119);
        assert_eq!(config.stun_port, 21116);
    }

    #[test]
    fn test_config_from_toml() {
        let toml = r#"
signaling_port = 9999
relay_port = 10000
stun_port = 10001
health_port = 10002
log_level = "debug"
max_message_size = 131072
rate_limit_per_second = 50
peer_timeout_secs = 120
"#;
        let config: ServerConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.signaling_port, 9999);
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.max_message_size, 131072);
    }

    #[test]
    fn test_config_validate_rejects_zero_port() {
        let mut config = ServerConfig::default();
        config.signaling_port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_apply_cli_overrides() {
        let mut config = ServerConfig::default();
        config.apply_cli_overrides(Some(8888), None, Some(9999), None);
        assert_eq!(config.signaling_port, 8888);
        assert_eq!(config.relay_port, 21119); // unchanged
        assert_eq!(config.stun_port, 9999);
    }

    #[test]
    fn test_config_from_file_roundtrip() {
        let dir = std::env::temp_dir().join("alldesk_test_config");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_config.toml");

        let original = ServerConfig {
            signaling_port: 7777,
            log_level: "trace".to_string(),
            ..Default::default()
        };

        let toml_str = toml::to_string_pretty(&original).unwrap();
        std::fs::write(&path, &toml_str).unwrap();

        let loaded = ServerConfig::from_file(&path).unwrap();
        assert_eq!(loaded.signaling_port, 7777);
        assert_eq!(loaded.log_level, "trace");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
