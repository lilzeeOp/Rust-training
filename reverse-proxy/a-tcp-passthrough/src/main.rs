//! Level 1 — Bare bones TCP passthrough.
//!
//! Hardcoded addresses. Accepts ONE connection then exits.
//! Manual read/write loop that can only move bytes in one direction at a time.
//! This is intentionally incomplete — it exists to show every line of the data path.

mod config;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// Hardcoded for Level 1. Level 2 will load these from config.toml.
const LISTEN_ADDR: &str = "127.0.0.1:8080";
const UPSTREAM_ADDR: &str = "127.0.0.1:9090";

#[tokio::main]
async fn main() {
    // Bind a TCP listener to our address.
    // Think of this as: open a door and wait for someone to knock.
    let listener = TcpListener::bind(LISTEN_ADDR).await.unwrap();
    println!("Listening on {LISTEN_ADDR}");

    // .await here yields to the Tokio executor until a client connects.
    // In Go this would be: conn, _ := listener.Accept()
    // The difference: Go's runtime parks the goroutine invisibly.
    // Tokio requires the explicit .await — same outcome, visible mechanism.
    let (mut client, client_addr) = listener.accept().await.unwrap();
    println!("Client connected: {client_addr}");

    // Connect to the upstream server.
    // .await again — yields until the TCP handshake completes.
    let mut upstream = TcpStream::connect(UPSTREAM_ADDR).await.unwrap();
    println!("Connected to upstream: {UPSTREAM_ADDR}");

    // A 4KB buffer on the stack. We reuse it every iteration.
    let mut buf = [0u8; 4096];

    // THE INTENTIONALLY NAIVE LOOP.
    //
    // Problem: this loop handles one direction per iteration.
    // Step 1: wait for client to send bytes → forward to upstream.
    // Step 2: wait for upstream to send bytes → forward to client.
    //
    // If upstream sends data WHILE we are blocked at step 1,
    // that data sits in the kernel receive buffer unread.
    // For simple request/response protocols (like HTTP) this can work.
    // For bidirectional protocols (like raw TCP piping) it breaks.
    // Level 2 fixes this with copy_bidirectional.
    loop {
        // Read from client. Returns 0 bytes when client closes the connection.
        let n = client.read(&mut buf).await.unwrap();
        if n == 0 {
            println!("Client closed connection");
            break;
        }
        println!("client→upstream: {n} bytes");
        upstream.write_all(&buf[..n]).await.unwrap();

        // Read from upstream.
        let n = upstream.read(&mut buf).await.unwrap();
        if n == 0 {
            println!("Upstream closed connection");
            break;
        }
        println!("upstream→client: {n} bytes");
        client.write_all(&buf[..n]).await.unwrap();
    }

    println!("Done");
}
