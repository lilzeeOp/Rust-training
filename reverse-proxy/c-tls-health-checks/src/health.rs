use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{debug, warn};

pub type HealthFlags = Arc<Vec<AtomicBool>>;

pub fn new_flags(n: usize) -> HealthFlags {
    Arc::new((0..n).map(|_| AtomicBool::new(true)).collect())
}

pub fn spawn_checkers(
    upstreams: Vec<SocketAddr>,
    flags: HealthFlags,
    interval: Duration,
    probe_timeout: Duration,
    path: String,
) {
    for (i, addr) in upstreams.into_iter().enumerate() {
        let flags = Arc::clone(&flags);
        let path = path.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let healthy = probe(addr, probe_timeout, &path).await;
                flags[i].store(healthy, Ordering::Relaxed);
                if healthy {
                    debug!(upstream = %addr, "health check OK");
                } else {
                    warn!(upstream = %addr, "health check FAILED — marking unhealthy");
                }
            }
        });
    }
}

async fn probe(addr: SocketAddr, probe_timeout: Duration, path: &str) -> bool {
    matches!(timeout(probe_timeout, do_probe(addr, path)).await, Ok(true))
}

async fn do_probe(addr: SocketAddr, path: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(addr).await else {
        return false;
    };
    let req = format!("GET {} HTTP/1.0\r\nHost: {}\r\n\r\n", path, addr);
    if stream.write_all(req.as_bytes()).await.is_err() {
        return false;
    }
    let mut buf = [0u8; 256];
    let n = match stream.read(&mut buf).await {
        Ok(n) if n > 0 => n,
        _ => return false,
    };
    let Ok(s) = std::str::from_utf8(&buf[..n]) else {
        return false;
    };
    s.starts_with("HTTP/")
        && s.split_whitespace()
            .nth(1)
            .map_or(false, |c| c.starts_with('2'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_marks_healthy_when_upstream_returns_200() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 256];
            let _ = conn.read(&mut buf).await;
            conn.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });
        let healthy = probe(addr, Duration::from_secs(2), "/health").await;
        assert!(healthy, "should be healthy on 200 response");
    }

    #[tokio::test]
    async fn test_marks_unhealthy_when_upstream_unreachable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let healthy = probe(addr, Duration::from_secs(2), "/health").await;
        assert!(!healthy, "should be unhealthy when unreachable");
    }

    #[tokio::test]
    async fn test_marks_unhealthy_when_upstream_returns_500() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 256];
            let _ = conn.read(&mut buf).await;
            conn.write_all(b"HTTP/1.0 500 Internal Server Error\r\n\r\n")
                .await
                .unwrap();
        });
        let healthy = probe(addr, Duration::from_secs(2), "/health").await;
        assert!(!healthy, "should be unhealthy on 500 response");
    }

    #[tokio::test]
    async fn test_flags_updated_by_spawn_checkers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                if let Ok((mut conn, _)) = listener.accept().await {
                    let mut buf = vec![0u8; 256];
                    let _ = conn.read(&mut buf).await;
                    let _ = conn
                        .write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n")
                        .await;
                }
            }
        });

        let flags = new_flags(1);
        flags[0].store(false, Ordering::Relaxed);

        spawn_checkers(
            vec![addr],
            Arc::clone(&flags),
            Duration::from_millis(50),
            Duration::from_secs(1),
            "/health".to_string(),
        );

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            flags[0].load(Ordering::Relaxed),
            "flag should be true after successful probe cycle"
        );
    }
}
