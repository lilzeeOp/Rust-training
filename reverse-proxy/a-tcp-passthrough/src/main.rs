//! Level 2 — Concurrent TCP passthrough.
//!
//! Config loaded from config.toml.
//! Each connection is handled in its own Tokio task (the go func() equivalent).
//! copy_bidirectional replaces the naive manual loop from Level 1.

mod config;
mod proxy;

use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize structured logging. RUST_LOG=info controls the verbosity.
    // Try: RUST_LOG=debug cargo run  for more detail.
    tracing_subscriber::fmt().init();

    // Load and validate config. Fails fast with a clear error if anything is wrong.
    let config = Arc::new(
        config::Config::load("config.toml").context("failed to load config.toml")?,
    );

    // Bind the listener.
    let listener = tokio::net::TcpListener::bind(config.listen_socket_addr())
        .await
        .with_context(|| format!("failed to bind to {}", config.listen_addr))?;

    info!(addr = %config.listen_addr, "proxy listening");

    // The accept loop. This runs forever — the main task never does real work,
    // it only hands connections off to spawned tasks.
    loop {
        // .await here yields until a client connects.
        let (client, addr) = listener
            .accept()
            .await
            .context("failed to accept connection")?;

        // Clone the Arc — cheap: just bumps a reference count.
        // Each task owns one reference to the same Config on the heap.
        let config = Arc::clone(&config);

        // tokio::spawn creates a new async task on the Tokio thread pool.
        // This is non-blocking — we return to the accept loop immediately.
        // Go equivalent: go handleConnection(conn, config)
        tokio::spawn(async move {
            if let Err(e) = proxy::handle_connection(client, config).await {
                error!(client = %addr, error = %e, "connection error");
            }
        });
    }
}
