//! Option B — HTTP/1.1 reverse proxy with round-robin load balancing.
//!
//! AWS-grade features:
//!   - X-Forwarded-For, X-Request-ID, Via header injection
//!   - 502 / 400 / 431 HTTP error responses (no TCP resets)
//!   - Structured request log: [req_id] METHOD /path → upstream in Xms
//!   - Graceful CTRL+C shutdown
//!   - Active connection counter

mod balancer;
mod config;
mod http;
mod proxy;

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
                .add_directive("b_http_load_balancer=info".parse().unwrap()),
        )
        .init();

    let config = Arc::new(
        config::Config::load("config.toml").context("failed to load config.toml")?,
    );

    let balancer = Arc::new(balancer::Balancer::new(config.upstream_socket_addrs()));

    let listener = tokio::net::TcpListener::bind(config.listen_socket_addr())
        .await
        .with_context(|| format!("failed to bind to {}", config.listen_addr))?;

    info!(
        addr = %config.listen_addr,
        upstreams = balancer.len(),
        "HTTP proxy listening"
    );

    let active = Arc::new(AtomicUsize::new(0));

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (client, addr) = result.context("failed to accept connection")?;
                let config   = Arc::clone(&config);
                let balancer = Arc::clone(&balancer);
                let active   = Arc::clone(&active);

                let count = active.fetch_add(1, Ordering::Relaxed) + 1;
                info!(client = %addr, active = count, "[NEW]");

                tokio::spawn(async move {
                    if let Err(e) = proxy::handle_connection(client, config, balancer).await {
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
