use anyhow::{bail, Context, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
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

pub async fn handle_connection(
    mut client: TcpStream,
    config: Arc<Config>,
    balancer: Arc<Balancer>,
) -> Result<()> {
    let start = Instant::now();
    let request_id = Uuid::new_v4().to_string();
    let client_addr = client
        .peer_addr()
        .context("failed to get client address")?;
    let client_ip = client_addr.ip().to_string();

    // Step 1: Read headers (up to 8KB)
    let mut header_buf = [0u8; HEADER_BUF_SIZE];
    let header_len = match read_until_headers_end(&mut client, &mut header_buf).await {
        Ok(n) => n,
        Err(_) => {
            let _ = client.write_all(RESPONSE_431).await;
            warn!(request_id = %&request_id[..8], "header overflow — sent 431");
            return Ok(());
        }
    };

    // Step 2: Pick upstream via round-robin
    let upstream_addr = balancer.next();

    // Step 3: Parse and rewrite headers
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

    // Step 4: Connect to upstream (with timeout)
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

    // Step 5: Write rewritten headers to upstream
    upstream
        .write_all(&rewritten)
        .await
        .context("failed to write headers to upstream")?;

    // Step 6: Pipe remaining body + full response bidirectionally
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

async fn read_until_headers_end(stream: &mut TcpStream, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    loop {
        if filled >= buf.len() {
            bail!("header buffer overflow: request headers exceed {} bytes", buf.len());
        }
        let n = stream
            .read(&mut buf[filled..])
            .await
            .context("failed to read from client")?;
        if n == 0 {
            bail!("client closed connection before completing headers");
        }
        filled += n;

        // Scan for \r\n\r\n — end of HTTP headers
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
    use crate::config::{Config, Timeouts};
    use tokio::net::TcpListener;

    fn make_config() -> Arc<Config> {
        Arc::new(Config {
            listen_addr: "127.0.0.1:0".to_string(),
            upstream_addrs: vec!["127.0.0.1:0".to_string()],
            timeouts: Timeouts {
                connect_timeout_secs: 5,
                transfer_timeout_secs: 10,
            },
        })
    }

    fn make_balancer(addr: &str) -> Arc<Balancer> {
        Arc::new(Balancer::new(vec![addr.parse().unwrap()]))
    }

    /// Verifies injected headers reach the upstream.
    #[tokio::test]
    async fn test_request_forwarded_with_injected_headers() {
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap().to_string();

        tokio::spawn(async move {
            let (mut conn, _) = upstream_listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = conn.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(req.contains("X-Forwarded-For:"), "missing X-Forwarded-For");
            assert!(req.contains("X-Request-ID:"), "missing X-Request-ID");
            assert!(req.contains("Via: 1.1 rust-proxy"), "missing Via");
            assert!(req.contains("Connection: close"), "missing Connection: close");
            conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        let client_task = tokio::spawn(async move {
            let mut client = TcpStream::connect(proxy_addr).await.unwrap();
            client
                .write_all(b"GET /hello HTTP/1.1\r\nHost: example.com\r\n\r\n")
                .await
                .unwrap();
            let mut buf = vec![0u8; 1024];
            let n = client.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(resp.contains("200 OK"), "expected 200 OK, got: {}", resp);
        });

        let (client_stream, _) = proxy_listener.accept().await.unwrap();
        handle_connection(client_stream, make_config(), make_balancer(&upstream_addr))
            .await
            .unwrap();
        client_task.await.unwrap();
    }

    /// Verifies client receives 502 when upstream is unreachable.
    #[tokio::test]
    async fn test_502_when_upstream_unreachable() {
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap().to_string();
        drop(upstream_listener);

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        let client_task = tokio::spawn(async move {
            let mut client = TcpStream::connect(proxy_addr).await.unwrap();
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n")
                .await
                .unwrap();
            let mut buf = vec![0u8; 1024];
            let n = client.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);
            assert!(resp.contains("502 Bad Gateway"), "expected 502, got: {}", resp);
        });

        let (client_stream, _) = proxy_listener.accept().await.unwrap();
        handle_connection(client_stream, make_config(), make_balancer(&upstream_addr))
            .await
            .unwrap();
        client_task.await.unwrap();
    }

    /// Verifies 3 requests are distributed one each across 3 upstreams.
    #[tokio::test]
    async fn test_round_robin_distributes_across_three_upstreams() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let counters: Vec<Arc<AtomicUsize>> =
            (0..3).map(|_| Arc::new(AtomicUsize::new(0))).collect();

        let mut upstream_addrs = Vec::new();
        for i in 0..3 {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            upstream_addrs.push(listener.local_addr().unwrap().to_string());
            let counter = Arc::clone(&counters[i]);
            tokio::spawn(async move {
                loop {
                    if let Ok((mut conn, _)) = listener.accept().await {
                        counter.fetch_add(1, Ordering::Relaxed);
                        let _ = conn
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                            .await;
                    }
                }
            });
        }

        let balancer = Arc::new(Balancer::new(
            upstream_addrs.iter().map(|a| a.parse().unwrap()).collect(),
        ));

        for _ in 0..3 {
            let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let proxy_addr = proxy_listener.local_addr().unwrap();

            let client_task = tokio::spawn(async move {
                let mut client = TcpStream::connect(proxy_addr).await.unwrap();
                client
                    .write_all(b"GET / HTTP/1.1\r\nHost: test\r\n\r\n")
                    .await
                    .unwrap();
                let mut buf = vec![0u8; 256];
                let _ = client.read(&mut buf).await;
            });

            let (client_stream, _) = proxy_listener.accept().await.unwrap();
            handle_connection(client_stream, make_config(), Arc::clone(&balancer))
                .await
                .unwrap();
            client_task.await.unwrap();
        }

        for (i, counter) in counters.iter().enumerate() {
            assert_eq!(
                counter.load(Ordering::Relaxed),
                1,
                "upstream {} should have received exactly 1 request",
                i
            );
        }
    }
}
