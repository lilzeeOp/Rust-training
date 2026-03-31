//! Option D — HTTPS reverse proxy with HTTP/2 support via hyper.
//!
//! AWS-grade features:
//!   - HTTP/1.1 and HTTP/2 via ALPN negotiation (TLS extension)
//!   - TLS 1.2/1.3 termination via rustls (self-signed cert auto-generated)
//!   - hyper as HTTP engine — typed Request/Response, streaming bodies
//!   - Forwards to backends over plain HTTP/1.1
//!   - Background health checker per upstream
//!   - Health-aware round-robin — 503 when all upstreams down
//!   - X-Forwarded-For, X-Forwarded-Proto: https, X-Request-ID, Via injection
//!   - Graceful CTRL+C shutdown
//!   - Active connection counter

mod balancer;
mod config;
mod health;
mod proxy;
mod tls;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use anyhow::{Context, Result};
use tracing::{error, info};

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("d_http2_hyper=info".parse().unwrap()),
        )
        .init();

    let config = Arc::new(
        config::Config::load("config.toml").context("failed to load config.toml")?,
    );

    let acceptor = Arc::new(
        tls::build_acceptor(&config.tls).context("failed to build TLS acceptor")?,
    );

    let flags = health::new_flags(config.upstream_addrs.len());

    health::spawn_checkers(
        config.upstream_socket_addrs(),
        Arc::clone(&flags),
        std::time::Duration::from_secs(config.health.interval_secs),
        std::time::Duration::from_secs(config.health.timeout_secs),
        config.health.path.clone(),
    );

    let balancer = Arc::new(balancer::Balancer::new(
        config.upstream_socket_addrs(),
        Arc::clone(&flags),
    ));

    let listener = tokio::net::TcpListener::bind(config.listen_socket_addr())
        .await
        .with_context(|| format!("failed to bind to {}", config.listen_addr))?;

    info!(
        addr = %config.listen_addr,
        upstreams = balancer.len(),
        "HTTPS/HTTP2 proxy listening"
    );

    let active = Arc::new(AtomicUsize::new(0));

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (tcp, addr) = result.context("failed to accept connection")?;
                let config   = Arc::clone(&config);
                let acceptor = Arc::clone(&acceptor);
                let balancer = Arc::clone(&balancer);
                let active   = Arc::clone(&active);

                let count = active.fetch_add(1, Ordering::Relaxed) + 1;
                info!(client = %addr, active = count, "[NEW]");

                tokio::spawn(async move {
                    if let Err(e) =
                        proxy::handle_connection(tcp, acceptor, config, balancer).await
                    {
                        error!(client = %addr, error = %e, "connection error");
                    }
                    let count = active.fetch_sub(1, Ordering::Relaxed) - 1;
                    info!(client = %addr, active = count, "[DONE]");
                });
            }
            _ = shutdown_signal() => {
                info!("shutdown signal received — stopping");
                break;
            }
        }
    }

    info!("proxy stopped");
    Ok(())
}
