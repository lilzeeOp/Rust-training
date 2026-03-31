use anyhow::{bail, Context, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};
use uuid::Uuid;

use crate::balancer::Balancer;
use crate::config::Config;
use crate::http;

const HEADER_BUF_SIZE: usize = 8192;

const RESPONSE_431: &[u8] =
    b"HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const RESPONSE_400: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const RESPONSE_502: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const RESPONSE_503: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

pub async fn handle_connection(
    tcp: TcpStream,
    acceptor: Arc<TlsAcceptor>,
    config: Arc<Config>,
    balancer: Arc<Balancer>,
) -> Result<()> {
    let start = Instant::now();
    let request_id = Uuid::new_v4().to_string();
    let client_addr = tcp.peer_addr().context("failed to get client address")?;
    let client_ip = client_addr.ip().to_string();

    // Step 1: TLS handshake (reuse connect_timeout_secs)
    let handshake_dur = Duration::from_secs(config.timeouts.connect_timeout_secs);
    let mut client = match timeout(handshake_dur, acceptor.accept(tcp)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            warn!(client = %client_addr, error = %e, "TLS handshake failed");
            return Ok(());
        }
        Err(_) => {
            warn!(client = %client_addr, "TLS handshake timeout");
            return Ok(());
        }
    };

    // Step 2: Read headers (up to 8KB)
    let mut header_buf = [0u8; HEADER_BUF_SIZE];
    let header_len = match read_until_headers_end(&mut client, &mut header_buf).await {
        Ok(n) => n,
        Err(_) => {
            let _ = client.write_all(RESPONSE_431).await;
            warn!(request_id = %&request_id[..8], "header overflow — sent 431");
            return Ok(());
        }
    };

    // Step 3: Pick healthy upstream
    let upstream_addr = match balancer.next_healthy() {
        Some(addr) => addr,
        None => {
            let _ = client.write_all(RESPONSE_503).await;
            warn!(request_id = %&request_id[..8], "all upstreams unhealthy — sent 503");
            return Ok(());
        }
    };

    // Step 4: Parse and rewrite headers
    let (rewritten, method, path) = match http::build_forwarded_request(
        &header_buf,
        header_len,
        &client_ip,
        &upstream_addr.to_string(),
        &request_id,
    ) {
        Ok(v) => v,
        Err(e) => {
            let _ = client.write_all(RESPONSE_400).await;
            warn!(request_id = %&request_id[..8], error = %e, "bad request — sent 400");
            return Ok(());
        }
    };

    // Step 5: Connect to upstream (with timeout)
    let connect_dur = Duration::from_secs(config.timeouts.connect_timeout_secs);
    let mut upstream = match timeout(connect_dur, TcpStream::connect(upstream_addr)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            let _ = client.write_all(RESPONSE_502).await;
            warn!(
                request_id = %&request_id[..8],
                upstream = %upstream_addr,
                error = %e,
                "upstream unreachable — sent 502"
            );
            return Ok(());
        }
        Err(_) => {
            let _ = client.write_all(RESPONSE_502).await;
            warn!(
                request_id = %&request_id[..8],
                upstream = %upstream_addr,
                "upstream connect timeout — sent 502"
            );
            return Ok(());
        }
    };

    // Step 6: Write rewritten headers to upstream
    upstream
        .write_all(&rewritten)
        .await
        .context("failed to write headers to upstream")?;

    // Step 7: Pipe remaining body + full response bidirectionally
    let transfer_dur = Duration::from_secs(config.timeouts.transfer_timeout_secs);
    let _ = timeout(
        transfer_dur,
        tokio::io::copy_bidirectional(&mut client, &mut upstream),
    )
    .await;

    let elapsed = start.elapsed().as_millis();
    info!(
        "[{}] {} {} → {} in {}ms",
        &request_id[..8],
        method,
        path,
        upstream_addr,
        elapsed
    );

    Ok(())
}

async fn read_until_headers_end<S>(stream: &mut S, buf: &mut [u8]) -> Result<usize>
where
    S: AsyncReadExt + Unpin,
{
    let mut filled = 0;
    loop {
        if filled >= buf.len() {
            bail!(
                "header buffer overflow: request headers exceed {} bytes",
                buf.len()
            );
        }
        let n = stream
            .read(&mut buf[filled..])
            .await
            .context("failed to read from client")?;
        if n == 0 {
            bail!("client closed connection before completing headers");
        }
        filled += n;
        if filled >= 4 {
            for i in 0..=(filled - 4) {
                if &buf[i..i + 4] == b"\r\n\r\n" {
                    return Ok(i + 4);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::balancer::Balancer;
    use crate::config::{Config, HealthConfig, Timeouts, TlsConfig};
    use crate::health::new_flags;
    use crate::tls::build_acceptor;
    use rustls::pki_types::ServerName;
    use std::sync::atomic::Ordering;
    use tokio::net::TcpListener;

    fn make_config(upstream: &str) -> Arc<Config> {
        Arc::new(Config {
            listen_addr: "127.0.0.1:0".to_string(),
            upstream_addrs: vec![upstream.to_string()],
            tls: TlsConfig {
                cert_path: String::new(),
                key_path: String::new(),
            },
            health: HealthConfig {
                interval_secs: 10,
                timeout_secs: 2,
                path: "/health".to_string(),
            },
            timeouts: Timeouts {
                connect_timeout_secs: 5,
                transfer_timeout_secs: 10,
            },
        })
    }

    fn make_acceptor() -> Arc<TlsAcceptor> {
        Arc::new(
            build_acceptor(&TlsConfig {
                cert_path: String::new(),
                key_path: String::new(),
            })
            .unwrap(),
        )
    }

    /// TLS client that skips certificate verification — for tests only.
    async fn tls_connect(addr: std::net::SocketAddr) -> tokio_rustls::client::TlsStream<TcpStream> {
        use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
        use rustls::pki_types::{CertificateDer, UnixTime};
        use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};

        #[derive(Debug)]
        struct NoVerify;
        impl ServerCertVerifier for NoVerify {
            fn verify_server_cert(
                &self,
                _: &CertificateDer<'_>,
                _: &[CertificateDer<'_>],
                _: &ServerName<'_>,
                _: &[u8],
                _: UnixTime,
            ) -> Result<ServerCertVerified, rustls::Error> {
                Ok(ServerCertVerified::assertion())
            }
            fn verify_tls12_signature(
                &self,
                _: &[u8],
                _: &CertificateDer<'_>,
                _: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }
            fn verify_tls13_signature(
                &self,
                _: &[u8],
                _: &CertificateDer<'_>,
                _: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }
            fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
                rustls::crypto::ring::default_provider()
                    .signature_verification_algorithms
                    .supported_schemes()
            }
        }

        let client_config = ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth();

        let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));
        let tcp = TcpStream::connect(addr).await.unwrap();
        let server_name = ServerName::try_from("localhost").unwrap();
        connector.connect(server_name, tcp).await.unwrap()
    }

    /// Verifies injected headers (including X-Forwarded-Proto) reach the upstream.
    #[tokio::test]
    async fn test_https_request_forwarded_to_plain_http_upstream() {
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut conn, _) = upstream_listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = conn.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(req.contains("X-Forwarded-For:"), "missing X-Forwarded-For");
            assert!(req.contains("X-Forwarded-Proto: https"), "missing X-Forwarded-Proto");
            assert!(req.contains("X-Request-ID:"), "missing X-Request-ID");
            assert!(req.contains("Via: 1.1 rust-proxy"), "missing Via");
            conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        let client_task = tokio::spawn(async move {
            let mut client = tls_connect(proxy_addr).await;
            client
                .write_all(b"GET /hello HTTP/1.1\r\nHost: example.com\r\n\r\n")
                .await
                .unwrap();
            let mut buf = vec![0u8; 1024];
            let n = client.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(resp.contains("200 OK"), "expected 200 OK, got: {}", resp);
        });

        let (tcp, _) = proxy_listener.accept().await.unwrap();
        let config = make_config(&upstream_addr.to_string());
        let flags = new_flags(1);
        let balancer = Arc::new(Balancer::new(config.upstream_socket_addrs(), flags));
        handle_connection(tcp, make_acceptor(), config, balancer)
            .await
            .unwrap();
        client_task.await.unwrap();
    }

    /// Verifies client receives 503 when all upstreams are marked unhealthy.
    #[tokio::test]
    async fn test_503_when_all_upstreams_unhealthy() {
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap().to_string();

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        let client_task = tokio::spawn(async move {
            let mut client = tls_connect(proxy_addr).await;
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n")
                .await
                .unwrap();
            let mut buf = vec![0u8; 1024];
            let n = client.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(
                resp.contains("503 Service Unavailable"),
                "expected 503, got: {}",
                resp
            );
        });

        let (tcp, _) = proxy_listener.accept().await.unwrap();
        let config = make_config(&upstream_addr);
        let flags = new_flags(1);
        flags[0].store(false, Ordering::Relaxed); // mark unhealthy
        let balancer = Arc::new(Balancer::new(config.upstream_socket_addrs(), flags));
        handle_connection(tcp, make_acceptor(), config, balancer)
            .await
            .unwrap();
        client_task.await.unwrap();
    }

    /// Verifies client receives 502 when upstream port is closed.
    #[tokio::test]
    async fn test_502_when_upstream_unreachable() {
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap().to_string();
        drop(upstream_listener); // close the port

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        let client_task = tokio::spawn(async move {
            let mut client = tls_connect(proxy_addr).await;
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n")
                .await
                .unwrap();
            let mut buf = vec![0u8; 1024];
            let n = client.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(
                resp.contains("502 Bad Gateway"),
                "expected 502, got: {}",
                resp
            );
        });

        let (tcp, _) = proxy_listener.accept().await.unwrap();
        let config = make_config(&upstream_addr);
        let flags = new_flags(1);
        let balancer = Arc::new(Balancer::new(config.upstream_socket_addrs(), flags));
        handle_connection(tcp, make_acceptor(), config, balancer)
            .await
            .unwrap();
        client_task.await.unwrap();
    }

    /// Verifies client receives 431 when it sends headers larger than 8KB.
    #[tokio::test]
    async fn test_431_on_header_overflow() {
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap().to_string();

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        let client_task = tokio::spawn(async move {
            let mut client = tls_connect(proxy_addr).await;
            // 9KB header with no \r\n\r\n — guaranteed to overflow 8KB buffer
            let big = format!("GET / HTTP/1.1\r\nX-Big: {}\r\n", "A".repeat(9000));
            client.write_all(big.as_bytes()).await.unwrap();
            let mut buf = vec![0u8; 1024];
            let n = client.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(resp.contains("431"), "expected 431, got: {}", resp);
        });

        let (tcp, _) = proxy_listener.accept().await.unwrap();
        let config = make_config(&upstream_addr);
        let flags = new_flags(1);
        let balancer = Arc::new(Balancer::new(config.upstream_socket_addrs(), flags));
        handle_connection(tcp, make_acceptor(), config, balancer)
            .await
            .unwrap();
        client_task.await.unwrap();
    }
}
