/// Example 4: Tokio I/O — TcpListener, AsyncRead/AsyncWrite, non-blocking I/O.
///
/// KEY CONCEPTS:
///   - TcpListener::bind().await registers the socket with the OS (epoll/kqueue/IOCP)
///   - listener.accept().await suspends without blocking the thread
///   - The runtime parks the thread and wakes it when the OS signals data is ready
///   - AsyncRead/AsyncWrite are the async versions of std::io Read/Write
///   - Each connection is handled in its own spawned task
///
/// RUN: cargo run --example 04_async_io
/// TEST: In another terminal: curl http://127.0.0.1:3456 or: echo "hello" | nc 127.0.0.1 3456
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    println!("=== Tokio Async I/O ===\n");
    println!("How TcpListener works under the hood:");
    println!("  1. bind()   → creates socket, registers with OS event system (IOCP on Windows)");
    println!("  2. accept() → suspends task (not thread!) until a connection arrives");
    println!("  3. read()   → suspends task until data is available on the socket");
    println!("  4. write()  → suspends task until the write buffer has space\n");

    let listener = TcpListener::bind("127.0.0.1:3456")
        .await
        .expect("failed to bind — is port 3456 in use?");

    println!("Listening on 127.0.0.1:3456");
    println!("Test with: curl http://127.0.0.1:3456");
    println!("Or:        echo \"hello\" | nc 127.0.0.1 3456");
    println!("Press Ctrl+C to stop.\n");

    loop {
        // .await here SUSPENDS this task — the thread is free to run other tasks
        // No thread is blocked! This is the key insight of async I/O.
        let (mut socket, addr) = listener.accept().await.unwrap();
        println!("[{addr}] connected");

        // Each connection gets its own task — thousands can run on a few threads
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];

            // Read what the client sends
            match socket.read(&mut buf).await {
                Ok(0) => {
                    println!("[{addr}] disconnected (read 0 bytes)");
                    return;
                }
                Ok(n) => {
                    let received = String::from_utf8_lossy(&buf[..n]);
                    println!("[{addr}] received {n} bytes: {}", received.trim());

                    // Echo it back with a prefix
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nEcho: {received}"
                    );
                    if let Err(e) = socket.write_all(response.as_bytes()).await {
                        println!("[{addr}] write error: {e}");
                    }
                }
                Err(e) => {
                    println!("[{addr}] read error: {e}");
                }
            }

            // Shutdown the write half — signals to the client that we're done
            let _ = socket.shutdown().await;
            println!("[{addr}] disconnected");
        });
    }

    // EXERCISE: Modify this to keep reading in a loop (not just one read per connection).
    // EXERCISE: Add a timeout — if a client is silent for 5 seconds, disconnect them.
    //           Hint: tokio::time::timeout(Duration::from_secs(5), socket.read(...))
}
