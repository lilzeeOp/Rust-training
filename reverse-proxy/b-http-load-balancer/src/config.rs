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
    pub upstream_addrs: Vec<String>,
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
        ensure!(!self.upstream_addrs.is_empty(), "upstream_addrs must not be empty");
        for addr in &self.upstream_addrs {
            addr.parse::<SocketAddr>()
                .with_context(|| format!("invalid upstream addr: {}", addr))?;
        }
        ensure!(
            self.timeouts.connect_timeout_secs > 0,
            "connect_timeout_secs must be greater than 0"
        );
        Ok(())
    }

    pub fn listen_socket_addr(&self) -> SocketAddr {
        self.listen_addr.parse().unwrap()
    }

    pub fn upstream_socket_addrs(&self) -> Vec<SocketAddr> {
        self.upstream_addrs.iter().map(|a| a.parse().unwrap()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_TOML: &str = r#"
        listen_addr = "127.0.0.1:8080"
        upstream_addrs = ["127.0.0.1:9091", "127.0.0.1:9092"]

        [timeouts]
        connect_timeout_secs  = 5
        transfer_timeout_secs = 60
    "#;

    #[test]
    fn test_parse_valid_config() {
        let config: Config = toml::from_str(VALID_TOML).unwrap();
        assert_eq!(config.listen_addr, "127.0.0.1:8080");
        assert_eq!(config.upstream_addrs.len(), 2);
        assert_eq!(config.upstream_addrs[0], "127.0.0.1:9091");
        assert_eq!(config.timeouts.connect_timeout_secs, 5);
    }

    #[test]
    fn test_validate_rejects_empty_upstreams() {
        let config: Config = toml::from_str(
            r#"
            listen_addr = "127.0.0.1:8080"
            upstream_addrs = []
            [timeouts]
            connect_timeout_secs  = 5
            transfer_timeout_secs = 60
        "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_invalid_upstream_addr() {
        let config: Config = toml::from_str(
            r#"
            listen_addr = "127.0.0.1:8080"
            upstream_addrs = ["not-an-addr"]
            [timeouts]
            connect_timeout_secs  = 5
            transfer_timeout_secs = 60
        "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_upstream_socket_addrs_returns_parsed_addrs() {
        let config: Config = toml::from_str(VALID_TOML).unwrap();
        let addrs = config.upstream_socket_addrs();
        assert_eq!(addrs.len(), 2);
        assert_eq!(addrs[0].port(), 9091);
        assert_eq!(addrs[1].port(), 9092);
    }
}
