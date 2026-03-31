use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

use crate::config::TlsConfig;

pub fn build_acceptor(cfg: &TlsConfig) -> Result<TlsAcceptor> {
    let (certs, key) = if cfg.cert_path.is_empty() {
        generate_self_signed()?
    } else {
        load_pem(&cfg.cert_path, &cfg.key_path)?
    };

    let server_config = ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .context("failed to set TLS protocol versions")?
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .context("failed to build TLS server config")?;

    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

fn generate_self_signed() -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .context("failed to generate self-signed cert")?;
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        cert.key_pair.serialize_der(),
    ));
    Ok((vec![cert_der], key_der))
}

fn load_pem(
    cert_path: &str,
    key_path: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_file = std::fs::File::open(cert_path)
        .with_context(|| format!("failed to open cert file: {}", cert_path))?;
    let mut cert_reader = std::io::BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse certificate PEM")?;

    let key_file = std::fs::File::open(key_path)
        .with_context(|| format!("failed to open key file: {}", key_path))?;
    let mut key_reader = std::io::BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .context("failed to parse private key PEM")?
        .context("no private key found in key file")?;

    Ok((certs, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_cfg() -> TlsConfig {
        TlsConfig {
            cert_path: String::new(),
            key_path: String::new(),
        }
    }

    #[test]
    fn test_generates_self_signed_cert() {
        let cfg = blank_cfg();
        assert!(build_acceptor(&cfg).is_ok(), "self-signed cert generation should succeed");
    }

    #[test]
    fn test_loads_pem_cert_and_key() {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let cert_pem = cert.cert.pem();
        let key_pem = cert.key_pair.serialize_pem();

        let dir = std::env::temp_dir();
        let cert_path = dir.join("c_proxy_test_cert.pem");
        let key_path = dir.join("c_proxy_test_key.pem");
        std::fs::write(&cert_path, cert_pem.as_bytes()).unwrap();
        std::fs::write(&key_path, key_pem.as_bytes()).unwrap();

        let cfg = TlsConfig {
            cert_path: cert_path.to_string_lossy().into_owned(),
            key_path: key_path.to_string_lossy().into_owned(),
        };
        assert!(build_acceptor(&cfg).is_ok(), "loading PEM cert+key should succeed");
    }
}
