use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize)]
pub struct Timeouts {
    pub connect_timeout_secs: u64,
    pub transfer_timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub listen_addr: String,
    pub upstream_addr: String,
    pub timeouts: Timeouts,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path))?;
        let config: Config =
            toml::from_str(&contents).context("failed to parse config file")?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        self.listen_addr
            .parse::<SocketAddr>()
            .with_context(|| format!("invalid listen_addr: {}", self.listen_addr))?;
        self.upstream_addr
            .parse::<SocketAddr>()
            .with_context(|| format!("invalid upstream_addr: {}", self.upstream_addr))?;
        ensure!(
            self.timeouts.connect_timeout_secs > 0,
            "connect_timeout_secs must be greater than 0"
        );
        Ok(())
    }

    pub fn listen_socket_addr(&self) -> SocketAddr {
        self.listen_addr.parse().unwrap()
    }

    pub fn upstream_socket_addr(&self) -> SocketAddr {
        self.upstream_addr.parse().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TOML: &str = r#"
        listen_addr   = "127.0.0.1:8080"
        upstream_addr = "127.0.0.1:9090"

        [timeouts]
        connect_timeout_secs  = 5
        transfer_timeout_secs = 60
    "#;

    #[test]
    fn test_parse_valid_config() {
        let config: Config = toml::from_str(VALID_TOML).unwrap();
        assert_eq!(config.listen_addr, "127.0.0.1:8080");
        assert_eq!(config.upstream_addr, "127.0.0.1:9090");
        assert_eq!(config.timeouts.connect_timeout_secs, 5);
        assert_eq!(config.timeouts.transfer_timeout_secs, 60);
    }

    #[test]
    fn test_validate_rejects_invalid_listen_addr() {
        let config: Config = toml::from_str(
            r#"
            listen_addr   = "not-a-valid-addr"
            upstream_addr = "127.0.0.1:9090"
            [timeouts]
            connect_timeout_secs  = 5
            transfer_timeout_secs = 60
        "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_zero_connect_timeout() {
        let config: Config = toml::from_str(
            r#"
            listen_addr   = "127.0.0.1:8080"
            upstream_addr = "127.0.0.1:9090"
            [timeouts]
            connect_timeout_secs  = 0
            transfer_timeout_secs = 60
        "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }
}
