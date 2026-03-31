use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use bytes::Bytes;
use hyper::http::header::{HeaderName, HeaderValue, HOST, VIA};
use http_body_util::{BodyExt, Empty};
use hyper::body::Incoming;
use hyper::client::conn::http1;
use hyper::server::conn::{http1 as server_http1, http2 as server_http2};
use hyper::service::Service;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};
use uuid::Uuid;

use crate::balancer::Balancer;
use crate::config::Config;

pub type BoxBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

fn empty_body() -> BoxBody {
    Empty::<Bytes>::new()
        .map_err(|_: Infallible| unreachable!())
        .boxed()
}

fn error_response(status: StatusCode) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-length", "0")
        .header("connection", "close")
        .body(empty_body())
        .unwrap()
}

#[derive(Clone)]
pub struct ProxyService {
    pub balancer: Arc<Balancer>,
    pub config: Arc<Config>,
    pub client_ip: String,
}

impl Service<Request<Incoming>> for ProxyService {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let this = self.clone();
        Box::pin(async move { Ok(this.handle(req).await) })
    }
}

impl ProxyService {
    async fn handle(&self, req: Request<Incoming>) -> Response<BoxBody> {
        let start = Instant::now();
        let request_id = Uuid::new_v4().to_string();
        let method = req.method().clone();
        let path = req.uri().path().to_string();

        // Pick healthy upstream
        let upstream_addr = match self.balancer.next_healthy() {
            Some(addr) => addr,
            None => {
                warn!(request_id = %&request_id[..8], "all upstreams unhealthy — sent 503");
                return error_response(StatusCode::SERVICE_UNAVAILABLE);
            }
        };

        // Build the forwarded request
        let forwarded = match build_forwarded_request(req, upstream_addr, &self.client_ip, &request_id) {
            Ok(r) => r,
            Err(e) => {
                warn!(request_id = %&request_id[..8], error = %e, "failed to build forwarded request — sent 502");
                return error_response(StatusCode::BAD_GATEWAY);
            }
        };

        // Connect to upstream
        let connect_dur = Duration::from_secs(self.config.timeouts.connect_timeout_secs);
        let stream = match timeout(connect_dur, TcpStream::connect(upstream_addr)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                warn!(request_id = %&request_id[..8], upstream = %upstream_addr, error = %e, "upstream unreachable — sent 502");
                return error_response(StatusCode::BAD_GATEWAY);
            }
            Err(_) => {
                warn!(request_id = %&request_id[..8], upstream = %upstream_addr, "upstream connect timeout — sent 502");
                return error_response(StatusCode::BAD_GATEWAY);
            }
        };

        // HTTP/1.1 handshake with upstream
        let (mut sender, conn) = match http1::Builder::new()
            .handshake(TokioIo::new(stream))
            .await
        {
            Ok(v) => v,
            Err(e) => {
                warn!(request_id = %&request_id[..8], error = %e, "http1 handshake failed — sent 502");
                return error_response(StatusCode::BAD_GATEWAY);
            }
        };
        tokio::spawn(conn);

        // Forward request
        let response = match sender.send_request(forwarded).await {
            Ok(r) => r,
            Err(e) => {
                warn!(request_id = %&request_id[..8], error = %e, "upstream request failed — sent 502");
                return error_response(StatusCode::BAD_GATEWAY);
            }
        };

        let elapsed = start.elapsed().as_millis();
        info!(
            "[{}] {} {} → {} in {}ms",
            &request_id[..8], method, path, upstream_addr, elapsed
        );

        // Stream the upstream response body back to the client
        response.map(|body| body.boxed())
    }
}

fn build_forwarded_request(
    req: Request<Incoming>,
    upstream_addr: SocketAddr,
    client_ip: &str,
    request_id: &str,
) -> Result<Request<Incoming>> {
    let (mut parts, body) = req.into_parts();

    // Strip hop-by-hop and anti-spoofing headers
    let strip: &[&str] = &[
        "host", "connection", "keep-alive", "proxy-connection",
        "te", "trailers", "transfer-encoding", "upgrade",
        "x-forwarded-for", "x-forwarded-proto", "x-request-id", "via",
    ];
    for name in strip {
        if let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) {
            parts.headers.remove(&header_name);
        }
    }

    // Rewrite URI to path-only (backend expects GET /path HTTP/1.1, not absolute URI)
    let path_and_query = parts.uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    parts.uri = path_and_query.parse()?;

    // Inject headers
    parts.headers.insert(HOST, HeaderValue::from_str(&upstream_addr.to_string())?);
    parts.headers.insert(
        HeaderName::from_static("x-forwarded-for"),
        HeaderValue::from_str(client_ip)?,
    );
    parts.headers.insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_static("https"),
    );
    parts.headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(request_id)?,
    );
    parts.headers.insert(VIA, HeaderValue::from_static("1.1 rust-proxy"));

    Ok(Request::from_parts(parts, body))
}

/// Full per-connection handler: TLS accept → ALPN detect → http1/http2 dispatch.
/// Called from main.rs accept loop and directly from integration tests.
pub async fn handle_connection(
    tcp: TcpStream,
    acceptor: Arc<TlsAcceptor>,
    config: Arc<Config>,
    balancer: Arc<Balancer>,
) -> Result<()> {
    let client_ip = tcp
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let handshake_dur = Duration::from_secs(config.timeouts.connect_timeout_secs);
    let tls_stream = match timeout(handshake_dur, acceptor.accept(tcp)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            warn!(error = %e, "TLS handshake failed");
            return Ok(());
        }
        Err(_) => {
            warn!("TLS handshake timeout");
            return Ok(());
        }
    };

    let alpn = tls_stream.get_ref().1.alpn_protocol().map(|p| p.to_vec());
    let service = ProxyService { balancer, config, client_ip };

    if alpn.as_deref() == Some(b"h2") {
        if let Err(e) = server_http2::Builder::new(TokioExecutor::new())
            .serve_connection(TokioIo::new(tls_stream), service)
            .await
        {
            warn!(error = %e, "http2 connection error");
        }
    } else {
        if let Err(e) = server_http1::Builder::new()
            .serve_connection(TokioIo::new(tls_stream), service)
            .await
        {
            warn!(error = %e, "http1 connection error");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::balancer::Balancer;
    use crate::config::{Config, HealthConfig, Timeouts, TlsConfig};
    use crate::health::new_flags;
    use crate::tls::build_acceptor;
    use http_body_util::Empty;
    use hyper::client::conn::http2 as client_http2;
    use rustls::pki_types::ServerName;
    use std::sync::atomic::Ordering;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn make_config(upstream: &str) -> Arc<Config> {
        Arc::new(Config {
            listen_addr: "127.0.0.1:0".to_string(),
            upstream_addrs: vec![upstream.to_string()],
            tls: TlsConfig { cert_path: String::new(), key_path: String::new() },
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
        Arc::new(build_acceptor(&TlsConfig { cert_path: String::new(), key_path: String::new() }).unwrap())
    }

    /// TLS client with configurable ALPN. Pass b"http/1.1" or b"h2".
    async fn tls_connect(
        addr: std::net::SocketAddr,
        alpn: &[u8],
    ) -> tokio_rustls::client::TlsStream<TcpStream> {
        use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
        use rustls::pki_types::{CertificateDer, UnixTime};
        use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};

        #[derive(Debug)]
        struct NoVerify;
        impl ServerCertVerifier for NoVerify {
            fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
                Ok(ServerCertVerified::assertion())
            }
            fn verify_tls12_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }
            fn verify_tls13_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }
            fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
                rustls::crypto::ring::default_provider()
                    .signature_verification_algorithms
                    .supported_schemes()
            }
        }

        let mut client_config = ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth();

        client_config.alpn_protocols = vec![alpn.to_vec()];

        let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));
        let tcp = TcpStream::connect(addr).await.unwrap();
        let server_name = ServerName::try_from("localhost").unwrap();
        connector.connect(server_name, tcp).await.unwrap()
    }

    /// Starts a plain HTTP/1.1 upstream that reads the request and responds 200 OK.
    async fn start_upstream() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        (listener, addr)
    }

    /// HTTP/1.1 client sends request; verifies injected headers reach the upstream.
    #[tokio::test]
    async fn test_http1_request_forwarded_with_injected_headers() {
        let (upstream_listener, upstream_addr) = start_upstream().await;
        tokio::spawn(async move {
            loop {
                if let Ok((mut conn, _)) = upstream_listener.accept().await {
                    let mut buf = vec![0u8; 4096];
                    let n = conn.read(&mut buf).await.unwrap();
                    let req = String::from_utf8_lossy(&buf[..n]);
                    assert!(req.contains("x-forwarded-for:") || req.contains("X-Forwarded-For:"),
                        "missing X-Forwarded-For in: {}", req);
                    assert!(req.contains("x-forwarded-proto: https") || req.contains("X-Forwarded-Proto: https"),
                        "missing X-Forwarded-Proto in: {}", req);
                    assert!(req.contains("x-request-id:") || req.contains("X-Request-ID:"),
                        "missing X-Request-ID in: {}", req);
                    assert!(req.contains("via:") || req.contains("Via:"),
                        "missing Via in: {}", req);
                    conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await.unwrap();
                }
            }
        });

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();
        let config = make_config(&upstream_addr.to_string());
        let flags = new_flags(1);
        let balancer = Arc::new(Balancer::new(config.upstream_socket_addrs(), Arc::clone(&flags)));

        let client_task = tokio::spawn(async move {
            let tls_stream = tls_connect(proxy_addr, b"http/1.1").await;
            let (mut sender, conn) = http1::Builder::new()
                .handshake(TokioIo::new(tls_stream))
                .await.unwrap();
            tokio::spawn(conn);
            let req = Request::builder()
                .method("GET").uri("/hello")
                .header("host", "example.com")
                .body(Empty::<Bytes>::new()).unwrap();
            let resp = sender.send_request(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "expected 200 OK");
        });

        let (tcp, _) = proxy_listener.accept().await.unwrap();
        handle_connection(tcp, make_acceptor(), config, balancer).await.unwrap();
        client_task.await.unwrap();
    }

    /// HTTP/2 client sends request; proxy negotiates h2 via ALPN and serves it.
    #[tokio::test]
    async fn test_http2_request_forwarded_with_injected_headers() {
        let (upstream_listener, upstream_addr) = start_upstream().await;
        tokio::spawn(async move {
            loop {
                if let Ok((mut conn, _)) = upstream_listener.accept().await {
                    let mut buf = vec![0u8; 4096];
                    let _ = conn.read(&mut buf).await;
                    conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await.unwrap();
                }
            }
        });

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();
        let config = make_config(&upstream_addr.to_string());
        let flags = new_flags(1);
        let balancer = Arc::new(Balancer::new(config.upstream_socket_addrs(), Arc::clone(&flags)));

        let client_task = tokio::spawn(async move {
            let tls_stream = tls_connect(proxy_addr, b"h2").await;
            let (mut sender, conn) = client_http2::Builder::new(TokioExecutor::new())
                .handshake(TokioIo::new(tls_stream))
                .await.unwrap();
            tokio::spawn(conn);
            let req = Request::builder()
                .method("GET").uri("/hello")
                .header("host", "example.com")
                .body(Empty::<Bytes>::new()).unwrap();
            let resp = sender.send_request(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "expected 200 OK via HTTP/2");
        });

        let (tcp, _) = proxy_listener.accept().await.unwrap();
        handle_connection(tcp, make_acceptor(), config, balancer).await.unwrap();
        client_task.await.unwrap();
    }

    /// All upstreams marked unhealthy → client receives 503.
    #[tokio::test]
    async fn test_503_when_all_upstreams_unhealthy() {
        let (upstream_listener, upstream_addr) = start_upstream().await;
        drop(upstream_listener);

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();
        let config = make_config(&upstream_addr.to_string());
        let flags = new_flags(1);
        flags[0].store(false, Ordering::Relaxed);
        let balancer = Arc::new(Balancer::new(config.upstream_socket_addrs(), Arc::clone(&flags)));

        let client_task = tokio::spawn(async move {
            let tls_stream = tls_connect(proxy_addr, b"http/1.1").await;
            let (mut sender, conn) = http1::Builder::new()
                .handshake(TokioIo::new(tls_stream)).await.unwrap();
            tokio::spawn(conn);
            let req = Request::builder()
                .method("GET").uri("/")
                .header("host", "example.com")
                .body(Empty::<Bytes>::new()).unwrap();
            let resp = sender.send_request(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE, "expected 503");
        });

        let (tcp, _) = proxy_listener.accept().await.unwrap();
        handle_connection(tcp, make_acceptor(), config, balancer).await.unwrap();
        client_task.await.unwrap();
    }

    /// Upstream port is closed → client receives 502.
    #[tokio::test]
    async fn test_502_when_upstream_unreachable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = listener.local_addr().unwrap().to_string();
        drop(listener);

        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy_listener.local_addr().unwrap();
        let config = make_config(&upstream_addr);
        let flags = new_flags(1);
        let balancer = Arc::new(Balancer::new(config.upstream_socket_addrs(), flags));

        let client_task = tokio::spawn(async move {
            let tls_stream = tls_connect(proxy_addr, b"http/1.1").await;
            let (mut sender, conn) = http1::Builder::new()
                .handshake(TokioIo::new(tls_stream)).await.unwrap();
            tokio::spawn(conn);
            let req = Request::builder()
                .method("GET").uri("/")
                .header("host", "example.com")
                .body(Empty::<Bytes>::new()).unwrap();
            let resp = sender.send_request(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_GATEWAY, "expected 502");
        });

        let (tcp, _) = proxy_listener.accept().await.unwrap();
        handle_connection(tcp, make_acceptor(), config, balancer).await.unwrap();
        client_task.await.unwrap();
    }
}
