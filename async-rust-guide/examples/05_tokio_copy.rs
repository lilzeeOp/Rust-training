/// Example 5: tokio::io::copy — the building block of your TCP proxy.
///
/// KEY CONCEPTS:
///   - tokio::io::copy(&mut reader, &mut writer) shuffles bytes between two async streams
///   - It reads from reader into an internal buffer, then writes to writer — in a loop
///   - For a TCP proxy, you need BIDIRECTIONAL copy: client→upstream AND upstream→client
///   - tokio::io::copy_bidirectional does both directions concurrently
///   - This is essentially what your a-tcp-passthrough proxy does!
///
/// RUN: cargo run --example 05_tokio_copy
/// TEST: In terminal 1: nc -l 9090  (or ncat -l 9090 on Windows)
///       In terminal 2: cargo run --example 05_tokio_copy
///       In terminal 3: nc 127.0.0.1 3457  (or ncat 127.0.0.1 3457)
///       Type in terminal 3 → appears in terminal 1 (and vice versa!)
use tokio::io;
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() {
    println!("=== tokio::io::copy — TCP Proxy Foundation ===\n");
    println!("This is a minimal TCP proxy: binds on :3457, forwards to :9090\n");
    println!("Setup:");
    println!("  Terminal 1: ncat -l 9090           (upstream server)");
    println!("  Terminal 2: cargo run --example 05_tokio_copy  (this proxy)");
    println!("  Terminal 3: ncat 127.0.0.1 3457    (client)\n");
    println!("Type in terminal 3 → proxied to terminal 1, and vice versa.\n");

    let listener = TcpListener::bind("127.0.0.1:3457")
        .await
        .expect("failed to bind :3457");

    println!("Proxy listening on 127.0.0.1:3457 → forwarding to 127.0.0.1:9090");
    println!("Press Ctrl+C to stop.\n");

    loop {
        let (client_stream, client_addr) = listener.accept().await.unwrap();
        println!("[{client_addr}] connected");

        tokio::spawn(async move {
            // Connect to upstream
            let upstream_stream = match TcpStream::connect("127.0.0.1:9090").await {
                Ok(s) => {
                    println!("[{client_addr}] connected to upstream :9090");
                    s
                }
                Err(e) => {
                    println!("[{client_addr}] failed to connect to upstream: {e}");
                    return;
                }
            };

            // Split both streams into read/write halves
            let (mut client_read, mut client_write) = client_stream.into_split();
            let (mut upstream_read, mut upstream_write) = upstream_stream.into_split();

            // Copy in both directions concurrently
            //
            // Under the hood, each copy() does:
            //   loop {
            //       let n = reader.read(&mut buf).await?;   // suspend until data
            //       if n == 0 { break; }                     // EOF
            //       writer.write_all(&buf[..n]).await?;      // suspend until written
            //   }
            //
            // By running both in parallel with tokio::join!, data flows both ways.
            let client_to_upstream = async {
                let bytes = io::copy(&mut client_read, &mut upstream_write).await;
                println!("[{client_addr}] client→upstream finished: {bytes:?}");
            };

            let upstream_to_client = async {
                let bytes = io::copy(&mut upstream_read, &mut client_write).await;
                println!("[{client_addr}] upstream→client finished: {bytes:?}");
            };

            // This is the core of a TCP passthrough proxy!
            tokio::join!(client_to_upstream, upstream_to_client);

            println!("[{client_addr}] proxy session ended");
        });
    }

    // EXERCISE: Add connect timeout using tokio::time::timeout
    // EXERCISE: Add transfer timeout — if no data flows for 60s, close the connection
    // EXERCISE: Replace the split + join! approach with io::copy_bidirectional()
    //           and compare: let (c2u, u2c) = io::copy_bidirectional(&mut client, &mut upstream).await?;
}
