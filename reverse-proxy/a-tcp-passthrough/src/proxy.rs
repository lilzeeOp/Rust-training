use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::info;

use crate::config::Config;

pub async fn handle_connection(mut client: TcpStream, config: Arc<Config>) -> Result<()> {
    let client_addr = client
        .peer_addr()
        .context("failed to get client peer address")?;

    // Connect to upstream.
    // If this fails, the error propagates up — the spawned task logs it and exits.
    // Every other connection is unaffected.
    let mut upstream = TcpStream::connect(config.upstream_socket_addr())
        .await
        .with_context(|| format!("failed to connect to upstream {}", config.upstream_addr))?;

    info!(
        client = %client_addr,
        upstream = %config.upstream_addr,
        "[NEW] connection"
    );

    // copy_bidirectional runs two copy loops concurrently inside a single task:
    //   loop 1: client.read → upstream.write
    //   loop 2: upstream.read → client.write
    //
    // It returns when EITHER side closes the connection (reads 0 bytes).
    // At that point it shuts down the write half of the other side — clean teardown.
    tokio::io::copy_bidirectional(&mut client, &mut upstream)
        .await
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
