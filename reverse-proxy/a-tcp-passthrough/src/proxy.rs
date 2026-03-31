use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::info;

use crate::config::Config;

pub async fn handle_connection(mut client: TcpStream, config: Arc<Config>) -> Result<()> {
    use std::time::Duration;
    use tokio::time::timeout;

    let client_addr = client
        .peer_addr()
        .context("failed to get client peer address")?;

    // Timeout on connect — if upstream doesn't accept within connect_timeout_secs, error out.
    let connect_timeout = Duration::from_secs(config.timeouts.connect_timeout_secs);
    let mut upstream = timeout(
        connect_timeout,
        TcpStream::connect(config.upstream_socket_addr()),
    )
    .await
    .with_context(|| {
        format!(
            "timed out connecting to upstream {} after {}s",
            config.upstream_addr, config.timeouts.connect_timeout_secs
        )
    })?
    .with_context(|| format!("failed to connect to upstream {}", config.upstream_addr))?;

    info!(
        client = %client_addr,
        upstream = %config.upstream_addr,
        "[NEW] connection"
    );

    // Timeout on transfer — kill connections that are idle/hung too long.
    let transfer_timeout = Duration::from_secs(config.timeouts.transfer_timeout_secs);
    timeout(
        transfer_timeout,
        tokio::io::copy_bidirectional(&mut client, &mut upstream),
    )
    .await
    .with_context(|| {
        format!(
            "transfer timed out after {}s for client {}",
            config.timeouts.transfer_timeout_secs, client_addr
        )
    })?
    .context("error during bidirectional copy")?;

    info!(client = %client_addr, "[CLOSED] connection");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Timeouts};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn make_config(upstream_addr: String) -> Arc<Config> {
        Arc::new(Config {
            listen_addr: "127.0.0.1:0".to_string(),
            upstream_addr,
            timeouts: Timeouts {
                connect_timeout_secs: 5,
                transfer_timeout_secs: 60,
            },
        })
    }

    /// Verifies bytes flow client → upstream AND upstream → client.
    #[tokio::test]
    async fn test_forwards_bytes_both_directions() {
        // Fake upstream: receives "hello", sends back "world"
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap().to_string();

        tokio::spawn(async move {
            let (mut conn, _) = upstream_listener.accept().await.unwrap();
            let mut buf = vec![0u8; 5];
            conn.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"hello");
            conn.write_all(b"world").await.unwrap();
        });

        // A listener so we can get a real TcpStream to pass to handle_connection
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        // Client task: sends "hello", expects "world" back
        let client_task = tokio::spawn(async move {
            let mut client = TcpStream::connect(proxy_addr).await.unwrap();
            client.write_all(b"hello").await.unwrap();
            let mut buf = vec![0u8; 5];
            client.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"world");
        });

        let (client_stream, _) = proxy_listener.accept().await.unwrap();
        handle_connection(client_stream, make_config(upstream_addr))
            .await
            .unwrap();

        client_task.await.unwrap();
    }

    /// Verifies the transfer timeout kills a connection that hangs idle.
    ///
    /// Without the timeout wrapper this test takes ~30 seconds (the upstream holds
    /// the connection open). With the timeout wrapper it finishes in ~1 second.
    #[tokio::test]
    async fn test_transfer_timeout_kills_idle_connection() {
        use std::time::{Duration, Instant};

        // Upstream accepts but never sends data — simulates a hung backend.
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (_conn, _) = upstream_listener.accept().await.unwrap();
            // Hold open for 30s — proxy must timeout long before this.
            tokio::time::sleep(Duration::from_secs(30)).await;
        });

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();
        let _client = tokio::spawn(async move {
            let _c = TcpStream::connect(proxy_addr).await.unwrap();
            tokio::time::sleep(Duration::from_secs(10)).await;
        });

        let config = Arc::new(Config {
            listen_addr: "127.0.0.1:0".to_string(),
            upstream_addr,
            timeouts: Timeouts {
                connect_timeout_secs: 5,
                transfer_timeout_secs: 1, // 1 second transfer timeout
            },
        });

        let (client_stream, _) = proxy_listener.accept().await.unwrap();
        let start = Instant::now();
        let result = handle_connection(client_stream, config).await;

        assert!(result.is_err(), "expected timeout error");
        assert!(
            start.elapsed() >= Duration::from_millis(900),
            "finished too fast — timeout may not have fired"
        );
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "took too long — timeout may not be wired up"
        );
    }

    /// Verifies handle_connection returns an error (doesn't panic) when upstream is down.
    #[tokio::test]
    async fn test_error_when_upstream_unreachable() {
        // Bind then immediately drop — port closed, connect will fail
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream_listener.local_addr().unwrap().to_string();
        drop(upstream_listener);

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();

        let _client_task = tokio::spawn(async move {
            let _c = TcpStream::connect(proxy_addr).await.unwrap();
        });

        let (client_stream, _) = proxy_listener.accept().await.unwrap();
        let result = handle_connection(client_stream, make_config(upstream_addr)).await;

        assert!(result.is_err(), "expected an error when upstream is unreachable");
    }
}
