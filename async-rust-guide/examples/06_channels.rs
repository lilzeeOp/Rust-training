// =============================================================
//  Chapter 6: CHANNELS — Sending Data Between Tasks
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 06_channels
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  When you have multiple tasks running, they need a way to talk
//  to each other. Channels are PIPES for data:
//    - One end is the SENDER (puts data in)
//    - Other end is the RECEIVER (takes data out)
//
//  Think of it like a mailbox:
//    - Workers put letters (messages) in the mailbox
//    - The boss checks the mailbox and reads them
//
//  TYPES OF CHANNELS:
//  -------------------
//    mpsc     = Many senders, one receiver (most common)
//    oneshot  = One sender, one receiver, ONE message only
//    broadcast = One sender, many receivers (everyone hears it)
//    watch    = One sender, many receivers, only latest value
//
// =============================================================

use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    // ===================================================
    //  DEMO 1: mpsc channel (many senders, one receiver)
    // ===================================================
    //
    // What happens:
    //   We create a channel with a buffer of 10 messages.
    //   We spawn 3 worker tasks. Each gets a CLONE of the sender.
    //   Workers do some "work" (sleep), then send a result.
    //   The main task receives results as they arrive.
    //
    // The channel closes when ALL senders are dropped.
    // That's why we must drop the original sender!

    println!("========================================");
    println!("  Demo 1: mpsc channel (many-to-one)");
    println!("========================================\n");

    // Create a channel. tx = sender (transmitter), rx = receiver
    // The 10 is the buffer size: up to 10 messages can be queued
    let (tx, mut rx) = mpsc::channel::<String>(10);

    for i in 1..=3 {
        let tx = tx.clone(); // Each worker gets its own copy of the sender
        tokio::spawn(async move {
            // Simulate doing different amounts of work
            sleep(Duration::from_millis(i * 300)).await;
            let msg = format!("Worker {} finished (took {}ms)", i, i * 300);
            println!("  [Worker {}] Sending result...", i);
            tx.send(msg).await.unwrap();
            // When this task ends, its `tx` clone is dropped automatically
        });
    }

    // IMPORTANT: Drop the original sender!
    // If we keep it alive, rx.recv() will wait FOREVER because
    // the channel only closes when ALL senders are dropped.
    drop(tx);

    // Receive results as they arrive
    println!("  [Main] Waiting for workers...\n");
    while let Some(message) = rx.recv().await {
        // rx.recv() returns:
        //   Some(msg) = a worker sent something
        //   None = all senders dropped, channel closed
        println!("  [Main] Got: {}", message);
    }
    println!("\n  [Main] Channel closed. All workers done.");

    // ===================================================
    //  DEMO 2: oneshot channel (one message, one time)
    // ===================================================
    //
    // What happens:
    //   We spawn a task to compute something.
    //   It sends back exactly ONE result through a oneshot channel.
    //   We await the result.
    //
    // Use this when you need a single answer from a background task.
    // Like asking someone a question and waiting for THE answer.

    println!("\n========================================");
    println!("  Demo 2: oneshot channel (one message)");
    println!("========================================\n");

    // oneshot: one sender, one receiver, one message
    let (tx, rx) = oneshot::channel::<u64>();

    tokio::spawn(async move {
        println!("  [Task] Computing fibonacci(20)...");
        sleep(Duration::from_millis(500)).await;

        // Simple fibonacci (not the best algorithm, but clear)
        fn fib(n: u64) -> u64 {
            if n <= 1 { return n; }
            fib(n - 1) + fib(n - 2)
        }

        let result = fib(20);
        println!("  [Task] Sending answer: {}", result);
        tx.send(result).unwrap(); // Send the ONE result
    });

    println!("  [Main] Waiting for answer...");
    let answer = rx.await.unwrap(); // Wait for the ONE result
    println!("  [Main] fibonacci(20) = {}", answer);

    // ===================================================
    //  DEMO 3: Real pattern — request/response with oneshot
    // ===================================================
    //
    // What happens:
    //   This is a very common pattern in Tokio programs:
    //   1. Main task sends a REQUEST through mpsc (with a oneshot sender inside)
    //   2. Worker receives the request, computes the answer
    //   3. Worker sends the RESPONSE back through the oneshot
    //
    //   This lets you have a "server" task that processes requests
    //   from multiple clients.

    println!("\n========================================");
    println!("  Demo 3: Request/Response pattern");
    println!("========================================\n");

    // A request contains: what to compute + where to send the answer
    struct Request {
        name: String,
        response_channel: oneshot::Sender<String>,
    }

    // Channel for sending requests to the server
    let (request_tx, mut request_rx) = mpsc::channel::<Request>(10);

    // Spawn a "server" task that processes requests
    tokio::spawn(async move {
        while let Some(req) = request_rx.recv().await {
            println!("  [Server] Processing request for '{}'...", req.name);
            sleep(Duration::from_millis(200)).await;
            let greeting = format!("Hello, {}! Welcome.", req.name);
            // Send the response back through the oneshot channel
            req.response_channel.send(greeting).unwrap();
        }
    });

    // Send 3 requests and get responses
    for name in ["Sujit", "Takashi", "Rust"] {
        // Create a oneshot for THIS request's response
        let (resp_tx, resp_rx) = oneshot::channel();

        // Send the request (including the response channel)
        request_tx
            .send(Request {
                name: name.to_string(),
                response_channel: resp_tx,
            })
            .await
            .unwrap();

        // Wait for the response
        let response = resp_rx.await.unwrap();
        println!("  [Client] Got response: {}", response);
    }

    println!("\n  Done!");
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  Demo 1 (mpsc):
//    - Worker 1 finishes first (300ms), Worker 2 (600ms), Worker 3 (900ms)
//    - Main receives results in the order they arrive
//    - Channel closes when last worker finishes (all senders dropped)
//
//  Demo 2 (oneshot):
//    - Task computes one value and sends it back
//    - Main gets exactly one result
//    - Simple and clean for single-response patterns
//
//  Demo 3 (request/response):
//    - Client sends request + a oneshot channel for the reply
//    - Server processes request, sends reply through the oneshot
//    - This pattern is EVERYWHERE in real Tokio programs
//
//  KEY TAKEAWAY:
//    Channels are how async tasks communicate.
//    mpsc = many messages from many senders
//    oneshot = one message, like a callback
// =============================================================
