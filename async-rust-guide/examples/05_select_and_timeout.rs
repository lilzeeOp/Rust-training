// =============================================================
//  Chapter 5: SELECT! AND TIMEOUT — Racing Futures
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 05_select_and_timeout
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  Sometimes you don't want to wait for ALL tasks. You want:
//    - The FIRST one to finish (and cancel the rest)
//    - A task to finish OR a timeout (whichever comes first)
//    - A task to finish OR a shutdown signal
//
//  tokio::select! does this. It races multiple futures and returns
//  as soon as ONE of them completes. The others are cancelled.
//
//  REAL-WORLD USES:
//  -----------------
//    - Send request to 3 mirror servers, use whichever responds first
//    - "Do this task, but give up after 5 seconds"
//    - "Keep processing requests, but stop if shutdown signal received"
//
// =============================================================

use tokio::time::{sleep, Duration, Instant};

async fn fast_server() -> String {
    sleep(Duration::from_secs(1)).await;
    "Response from FAST server".to_string()
}

async fn slow_server() -> String {
    sleep(Duration::from_secs(5)).await;
    "Response from SLOW server".to_string()
}

async fn very_slow_database() -> String {
    sleep(Duration::from_secs(10)).await;
    "database result".to_string()
}

#[tokio::main]
async fn main() {
    // ===================================================
    //  DEMO 1: Racing two servers
    // ===================================================
    //
    // What happens:
    //   select! starts both fast_server (1s) and slow_server (5s).
    //   After 1 second, fast_server finishes.
    //   select! returns immediately with the fast server's response.
    //   slow_server is CANCELLED — its future is dropped.
    //   Total time: ~1 second (not 5).

    println!("========================================");
    println!("  Demo 1: Racing two servers");
    println!("========================================\n");

    let start = Instant::now();

    tokio::select! {
        response = fast_server() => {
            println!("  Winner: {}", response);
        }
        response = slow_server() => {
            println!("  Winner: {}", response);
        }
    }

    println!("  Time: {:?}  (fast server won after ~1s)\n", start.elapsed());

    // ===================================================
    //  DEMO 2: Timeout with select!
    // ===================================================
    //
    // What happens:
    //   We race the slow database (10s) against a 2-second timer.
    //   The timer wins (2s < 10s).
    //   The database query is cancelled.
    //   We print a timeout message.

    println!("========================================");
    println!("  Demo 2: Timeout with select!");
    println!("========================================\n");

    let start = Instant::now();

    tokio::select! {
        result = very_slow_database() => {
            println!("  Database returned: {}", result);
        }
        _ = sleep(Duration::from_secs(2)) => {
            // The _ means we don't care about sleep's return value (it's ())
            println!("  TIMEOUT! Database took too long. Gave up after 2 seconds.");
        }
    }

    println!("  Time: {:?}\n", start.elapsed());

    // ===================================================
    //  DEMO 3: tokio::time::timeout (cleaner version)
    // ===================================================
    //
    // What happens:
    //   tokio::time::timeout is a helper that does the same thing
    //   as the select! pattern above but with nicer syntax.
    //   It returns Ok(result) if the task finishes in time,
    //   or Err if the timeout expired.

    println!("========================================");
    println!("  Demo 3: tokio::time::timeout helper");
    println!("========================================\n");

    let start = Instant::now();

    match tokio::time::timeout(Duration::from_secs(2), very_slow_database()).await {
        Ok(result) => {
            println!("  Success: {}", result);
        }
        Err(_) => {
            println!("  TIMEOUT! Gave up after 2 seconds.");
        }
    }

    println!("  Time: {:?}\n", start.elapsed());

    // ===================================================
    //  DEMO 4: select! in a loop (common pattern)
    // ===================================================
    //
    // What happens:
    //   We have a "worker" that produces values and a "shutdown" timer.
    //   We keep receiving values from the worker until the shutdown fires.
    //   This pattern is used in real servers for graceful shutdown.

    println!("========================================");
    println!("  Demo 4: select! in a loop (shutdown)");
    println!("========================================\n");

    let start = Instant::now();
    let mut count = 0;

    // Pin the shutdown future so we can use it in the loop.
    // (We'll learn about Pin later — for now, just know this is needed
    //  when using the same future across multiple select! calls)
    let shutdown = sleep(Duration::from_secs(3));
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            // Keep doing work every 500ms
            _ = sleep(Duration::from_millis(500)) => {
                count += 1;
                println!("  Processed item #{}", count);
            }
            // But stop when shutdown timer fires
            _ = &mut shutdown => {
                println!("\n  Shutdown signal received! Stopping after {} items.", count);
                break;
            }
        }
    }

    println!("  Time: {:?}", start.elapsed());
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  Demo 1: Fast server wins after ~1s. Slow server is cancelled.
//
//  Demo 2: Timeout fires after 2s. Database query (10s) is cancelled.
//
//  Demo 3: Same as Demo 2 but cleaner syntax with timeout().
//
//  Demo 4: Worker processes items every 500ms. After 3 seconds,
//           the shutdown timer fires and the loop breaks.
//           You see ~5-6 items processed before shutdown.
//
//  KEY TAKEAWAY:
//    select! = "run these futures, return the FIRST one to finish"
//    Extremely useful for timeouts, shutdown signals, and racing.
// =============================================================
