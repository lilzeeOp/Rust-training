// =============================================================
//  Chapter 8: REAL-WORLD — TCP Echo Server
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 08_tcp_echo_server
//
//  Then in ANOTHER terminal, test it:
//      echo "hello" | nc localhost 8080        (Linux/Mac)
//      curl telnet://localhost:8080            (or use any TCP client)
//
//  Or just watch the built-in test client run automatically!
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  We build a TCP echo server that:
//    1. Listens for connections
//    2. When someone connects, reads what they send
//    3. Sends it back (echo)
//    4. Handles MANY clients at the same time
//
//  This is directly relevant to your reverse proxy project.
//  A reverse proxy is this same pattern, except instead of
//  echoing data back, it FORWARDS data to another server.
//
//  CONCEPTS USED:
//    - TcpListener (listen for connections)
//    - tokio::spawn (one task per client)
//    - AsyncReadExt / AsyncWriteExt (async reading/writing)
//    - select! (shutdown after timeout)
//
// =============================================================

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    println!("========================================");
    println!("  TCP Echo Server + Test Client");
    println!("========================================\n");

    // Start the server in a background task
    let server = tokio::spawn(async {
        run_server().await;
    });

    // Give the server a moment to start
    sleep(Duration::from_millis(100)).await;

    // Run test clients to demonstrate
    run_test_clients().await;

    // Wait a bit then stop
    sleep(Duration::from_millis(500)).await;
    println!("\n  Demo complete!");

    // Cancel the server
    server.abort();
}

async fn run_server() {
    // Step 1: BIND to a port
    // This tells the OS: "I want to receive connections on port 8080"
    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    println!("  [Server] Listening on 127.0.0.1:8080");

    loop {
        // Step 2: ACCEPT a connection
        // This .await pauses until someone connects.
        // While waiting, other tasks (other clients) keep running!
        let (mut socket, addr) = listener.accept().await.unwrap();
        println!("  [Server] New client connected: {}", addr);

        // Step 3: SPAWN a task for this client
        // This is the key pattern! Each client gets its own task.
        // The accept loop immediately goes back to waiting for more clients.
        // This means 1000 clients can be connected at the same time!
        tokio::spawn(async move {
            let mut buf = [0u8; 1024]; // Buffer for reading data

            loop {
                // Step 4: READ data from the client
                // socket.read() is async — while waiting for data,
                // other client tasks can run!
                let n = match socket.read(&mut buf).await {
                    Ok(0) => {
                        // 0 bytes means client disconnected
                        println!("  [Server] Client {} disconnected", addr);
                        return;
                    }
                    Ok(n) => n, // n bytes were read
                    Err(e) => {
                        println!("  [Server] Error reading from {}: {}", addr, e);
                        return;
                    }
                };

                let received = String::from_utf8_lossy(&buf[..n]);
                println!("  [Server] From {}: '{}'", addr, received.trim());

                // Step 5: WRITE (echo) data back to the client
                if let Err(e) = socket.write_all(&buf[..n]).await {
                    println!("  [Server] Error writing to {}: {}", addr, e);
                    return;
                }
                println!("  [Server] Echoed back to {}", addr);
            }
        });
    }
}

async fn run_test_clients() {
    println!("\n  --- Running test clients ---\n");

    // Spawn 3 test clients concurrently
    let mut handles = Vec::new();

    for i in 1..=3 {
        let handle = tokio::spawn(async move {
            // Small delay so clients connect in order
            sleep(Duration::from_millis(i * 100)).await;

            // Connect to our server
            let mut stream =
                tokio::net::TcpStream::connect("127.0.0.1:8080").await.unwrap();

            let message = format!("Hello from client {}!\n", i);
            println!("  [Client {}] Sending: '{}'", i, message.trim());

            // Send a message
            stream.write_all(message.as_bytes()).await.unwrap();

            // Read the echo back
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let response = String::from_utf8_lossy(&buf[..n]);
            println!("  [Client {}] Received echo: '{}'", i, response.trim());
        });
        handles.push(handle);
    }

    // Wait for all clients to finish
    for handle in handles {
        handle.await.unwrap();
    }
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  1. The server starts listening
//  2. Three clients connect (almost at the same time!)
//  3. Each client sends a message
//  4. The server echoes each message back
//  5. Each client receives its echo
//
//  KEY THINGS TO SEE:
//    - Multiple clients are handled CONCURRENTLY
//    - Each client has its own spawned task
//    - The server accept loop is never blocked by any single client
//    - All reads and writes use .await (non-blocking)
//
//  FOR YOUR REVERSE PROXY:
//    - Same pattern: accept connection, spawn task per client
//    - Instead of echoing, you'd CONNECT to a backend server
//    - Then forward bytes: client -> backend and backend -> client
//    - That's basically what tokio::io::copy does (next chapter!)
// =============================================================
