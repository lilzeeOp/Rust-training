// =============================================================
//  Chapter 9: FULL PROJECT — Concurrent Health Checker
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 09_full_project
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  This is the "final exam" — a real program that uses EVERYTHING
//  you've learned:
//
//    - async fn and .await      (Chapter 1)
//    - tokio::spawn             (Chapter 2)
//    - Futures and polling      (Chapter 3 — behind the scenes)
//    - select! and timeout      (Chapter 5)
//    - mpsc channels            (Chapter 6)
//
//  We build a concurrent URL health checker that:
//    1. Takes a list of URLs (simulated)
//    2. Checks ALL of them at the same time
//    3. Each check has a timeout (won't wait forever)
//    4. Workers send results through a channel
//    5. Prints a nice summary table
//
// =============================================================

use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Duration, Instant};

/// Simulates checking if a URL is healthy.
/// In a real program, this would make an HTTP request.
async fn check_url(_url: &str, simulated_latency_ms: u64, will_fail: bool) -> Result<u16, String> {
    sleep(Duration::from_millis(simulated_latency_ms)).await;
    if will_fail {
        Err(format!("Connection refused"))
    } else {
        Ok(200) // HTTP 200 OK
    }
}

/// The result of checking one URL
struct HealthResult {
    url: String,
    status: String,
    duration_ms: u128,
}

#[tokio::main]
async fn main() {
    println!("=============================================");
    println!("  Concurrent URL Health Checker");
    println!("=============================================\n");

    let overall_start = Instant::now();

    // Our list of URLs to check:
    //   (url,                  latency_ms,  will_fail)
    let urls = vec![
        ("https://google.com",      200,  false),
        ("https://github.com",      500,  false),
        ("https://slow-site.com",   3000, false),  // Will TIMEOUT (>1s)
        ("https://broken.com",      100,  true),   // Will FAIL
        ("https://rust-lang.org",   300,  false),
        ("https://tokio.rs",        400,  false),
        ("https://dead-link.com",   150,  true),   // Will FAIL
        ("https://crates.io",       250,  false),
    ];

    let total_urls = urls.len();

    // Create a channel for workers to send results back
    let (tx, mut rx) = mpsc::channel::<HealthResult>(total_urls);

    println!("  Checking {} URLs concurrently (1s timeout each)...\n", total_urls);

    // Spawn one task per URL — they ALL run at the same time!
    for (url, latency, will_fail) in urls {
        let tx = tx.clone(); // Each task gets its own sender
        let url_string = url.to_string();

        tokio::spawn(async move {
            let start = Instant::now();

            // Check the URL with a 1-second timeout
            let status = match timeout(
                Duration::from_secs(1),
                check_url(&url_string, latency, will_fail),
            )
            .await
            {
                // timeout() returns Ok(inner_result) if it finished in time
                Ok(Ok(code)) => format!("OK ({})", code),
                Ok(Err(e)) => format!("FAIL - {}", e),
                // timeout() returns Err if time ran out
                Err(_) => "TIMEOUT (>1s)".to_string(),
            };

            let result = HealthResult {
                url: url_string,
                status,
                duration_ms: start.elapsed().as_millis(),
            };

            // Send result through the channel
            tx.send(result).await.ok();
        });
    }

    // Drop original sender so the channel closes when all tasks finish
    drop(tx);

    // Collect ALL results
    let mut results = Vec::new();
    while let Some(result) = rx.recv().await {
        results.push(result);
    }

    // Sort by URL name for a clean display
    results.sort_by(|a, b| a.url.cmp(&b.url));

    // Print the results table
    println!("  {:<28} {:<22} {:>8}", "URL", "STATUS", "TIME");
    println!("  {}", "-".repeat(60));

    let mut ok_count = 0;
    let mut fail_count = 0;
    let mut timeout_count = 0;

    for r in &results {
        let indicator = if r.status.starts_with("OK") {
            ok_count += 1;
            "[OK]"
        } else if r.status.starts_with("TIMEOUT") {
            timeout_count += 1;
            "[!!]"
        } else {
            fail_count += 1;
            "[XX]"
        };
        println!(
            "  {} {:<24} {:<22} {:>5}ms",
            indicator, r.url, r.status, r.duration_ms
        );
    }

    // Print summary
    println!("\n  =========== Summary ===========");
    println!("  Healthy:   {}", ok_count);
    println!("  Failed:    {}", fail_count);
    println!("  Timed out: {}", timeout_count);
    println!("  Total:     {}/{}", ok_count, total_urls);
    println!(
        "\n  Total time: {:?}  (all {} URLs checked concurrently!)",
        overall_start.elapsed(),
        total_urls
    );

    // Show the math
    println!("\n  Without async (sequential): ~4900ms (sum of all latencies)");
    println!(
        "  With async (concurrent):   ~{:?} (only as slow as the timeout!)",
        overall_start.elapsed()
    );
    println!("  Speedup: ~{}x faster!", 4900 / overall_start.elapsed().as_millis().max(1));
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  1. All 8 URLs are checked AT THE SAME TIME
//  2. The fast ones finish quickly (~100-500ms)
//  3. The slow one (slow-site.com, 3000ms) TIMES OUT at 1 second
//  4. The broken ones FAIL immediately
//  5. Total time is ~1 second (the timeout), NOT ~5 seconds
//
//  CONCEPTS DEMONSTRATED:
//    [async fn]       check_url() is async
//    [tokio::spawn]   One task per URL, all concurrent
//    [.await]         Every I/O operation yields to the runtime
//    [mpsc channel]   Workers send results to main task
//    [timeout]        Slow URLs cancelled after 1 second
//    [move closures]  Each task owns its data
//
//  THIS IS THE PATTERN FOR YOUR REVERSE PROXY:
//    - Accept connections (like our URL list)
//    - Spawn a task per connection (like our per-URL tasks)
//    - Forward data with timeouts (like our health checks)
//    - Report results (like our summary table)
// =============================================================
