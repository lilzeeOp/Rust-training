//! Level 3 — Production grade TCP passthrough.
//!
//! Adds: connect/transfer timeouts, graceful CTRL+C shutdown,
//! active connection counter, structured logging with env-filter.

mod config;
mod proxy;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use anyhow::{Context, Result};
use tracing::{error, info};

/// Resolves when CTRL+C is received.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

#[tokio::main]
async fn main() -> Result<()> {
    // env-filter reads RUST_LOG at startup.
    // Example: RUST_LOG=a_tcp_passthrough=debug cargo run
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("a_tcp_passthrough=info".parse().unwrap()),
        )
        .init();

    let config = Arc::new(
        config::Config::load("config.toml").context("failed to load config.toml")?,
    );

    let listener = tokio::net::TcpListener::bind(config.listen_socket_addr())
        .await
        .with_context(|| format!("failed to bind to {}", config.listen_addr))?;

    info!(addr = %config.listen_addr, "proxy listening");

    // Shared atomic counter — no Mutex needed for a simple increment/decrement.
    // Arc lets us clone a reference cheaply into each spawned task.
    let active = Arc::new(AtomicUsize::new(0));

    loop {
        tokio::select! {
            // Branch 1: a new client connection arrived.
            result = listener.accept() => {
                let (client, addr) = result.context("failed to accept connection")?;
                let config = Arc::clone(&config);
                let active = Arc::clone(&active);

                // Increment before spawn so the count is accurate immediately.
                let count = active.fetch_add(1, Ordering::Relaxed) + 1;
                info!(client = %addr, active = count, "[NEW] connection");

                tokio::spawn(async move {
                    if let Err(e) = proxy::handle_connection(client, config).await {
                        error!(client = %addr, error = %e, "connection error");
                    }
                    // fetch_sub returns the value BEFORE subtracting, so subtract 1 for current.
                    let count = active.fetch_sub(1, Ordering::Relaxed) - 1;
                    info!(client = %addr, active = count, "[DONE] connection");
                });
            }

            // Branch 2: CTRL+C received — stop accepting, let existing tasks finish.
            _ = shutdown_signal() => {
                info!("shutdown signal received — stopping");
                break;
            }
        }
    }

    info!("proxy stopped");
    Ok(())
}
