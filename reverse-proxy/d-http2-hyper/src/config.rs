use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize)]
pub struct Timeouts {
    pub connect_timeout_secs: u64,
    pub transfer_timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Deserialize)]
pub struct HealthConfig {
    pub interval_secs: u64,
    pub timeout_secs: u64,
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub listen_addr: String,
    pub upstream_addrs: Vec<String>,
    pub tls: TlsConfig,
    pub health: HealthConfig,
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
        ensure!(self.health.interval_secs > 0, "health.interval_secs must be > 0");
        ensure!(self.health.timeout_secs > 0, "health.timeout_secs must be > 0");
        ensure!(
            self.timeouts.connect_timeout_secs > 0,
            "connect_timeout_secs must be > 0"
        );
        match (self.tls.cert_path.is_empty(), self.tls.key_path.is_empty()) {
            (true, false) | (false, true) => anyhow::bail!(
                "tls.cert_path and tls.key_path must both be provided or both be empty"
            ),
            _ => {}
        }
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
        listen_addr    = "127.0.0.1:8443"
        upstream_addrs = ["127.0.0.1:9091", "127.0.0.1:9092"]
        [tls]
        cert_path = ""
        key_path  = ""
        [health]
        interval_secs = 10
        timeout_secs  = 2
        path          = "/health"
        [timeouts]
        connect_timeout_secs  = 5
        transfer_timeout_secs = 60
    "#;

    #[test]
    fn test_parse_valid_config() {
        let config: Config = toml::from_str(VALID_TOML).unwrap();
        assert_eq!(config.listen_addr, "127.0.0.1:8443");
        assert_eq!(config.upstream_addrs.len(), 2);
        assert_eq!(config.health.interval_secs, 10);
        assert_eq!(config.health.path, "/health");
    }

    #[test]
    fn test_validate_rejects_empty_upstreams() {
        let config: Config = toml::from_str(r#"
            listen_addr = "127.0.0.1:8443"
            upstream_addrs = []
            [tls]
            cert_path = ""
            key_path  = ""
            [health]
            interval_secs = 10
            timeout_secs  = 2
            path = "/health"
            [timeouts]
            connect_timeout_secs  = 5
            transfer_timeout_secs = 60
        "#).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_invalid_upstream_addr() {
        let config: Config = toml::from_str(r#"
            listen_addr = "127.0.0.1:8443"
            upstream_addrs = ["not-an-addr"]
            [tls]
            cert_path = ""
            key_path  = ""
            [health]
            interval_secs = 10
            timeout_secs  = 2
            path = "/health"
            [timeouts]
            connect_timeout_secs  = 5
            transfer_timeout_secs = 60
        "#).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_zero_health_interval() {
        let config: Config = toml::from_str(r#"
            listen_addr = "127.0.0.1:8443"
            upstream_addrs = ["127.0.0.1:9091"]
            [tls]
            cert_path = ""
            key_path  = ""
            [health]
            interval_secs = 0
            timeout_secs  = 2
            path = "/health"
            [timeouts]
            connect_timeout_secs  = 5
            transfer_timeout_secs = 60
        "#).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_mismatched_tls_paths() {
        let config: Config = toml::from_str(r#"
            listen_addr = "127.0.0.1:8443"
            upstream_addrs = ["127.0.0.1:9091"]
            [tls]
            cert_path = "/some/cert.pem"
            key_path  = ""
            [health]
            interval_secs = 10
            timeout_secs  = 2
            path = "/health"
            [timeouts]
            connect_timeout_secs  = 5
            transfer_timeout_secs = 60
        "#).unwrap();
        assert!(config.validate().is_err());
    }
}
