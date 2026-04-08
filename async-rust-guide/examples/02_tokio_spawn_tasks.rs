// =============================================================
//  Chapter 2: TOKIO::SPAWN — Background Tasks
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 02_tokio_spawn_tasks
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  tokio::join! waits for ALL tasks to finish before moving on.
//  tokio::spawn starts a task in the BACKGROUND — you keep working
//  and check on it later.
//
//  Think of it like this:
//    join!  = "Do these 3 things. I'll sit here until all 3 are done."
//    spawn  = "Go do this in the background. I'll keep doing my stuff."
//
//  IMPORTANT RULE: The 'static + move thing
//  -----------------------------------------
//  Spawned tasks must OWN all their data. You can't borrow from outside
//  because the task might live longer than the data you borrowed.
//  Use the `move` keyword to give ownership to the task.
//
// =============================================================

use tokio::time::{sleep, Duration, Instant};

#[tokio::main]
async fn main() {
    // ===================================================
    //  DEMO 1: Spawning background tasks
    // ===================================================
    //
    // What happens:
    //   We spawn 3 "download" tasks. They ALL start immediately.
    //   While they run, the main task does its own work.
    //   Then we collect results from each task.
    //
    // The key: the main task doesn't sit and wait for downloads.
    // It keeps doing useful work while downloads happen in the background.

    println!("========================================");
    println!("  Demo 1: Background downloads");
    println!("========================================\n");

    let start = Instant::now();

    // Spawn task 1 — starts running in the background immediately
    let task1 = tokio::spawn(async {
        println!("  [{:.1?}] Download A: starting (will take 3s)...", Instant::now());
        sleep(Duration::from_secs(3)).await;
        println!("  [{:.1?}] Download A: DONE!", Instant::now());
        1000 // return "file size" in bytes
    });

    // Spawn task 2 — also starts immediately (doesn't wait for task 1)
    let task2 = tokio::spawn(async {
        println!("  [{:.1?}] Download B: starting (will take 1s)...", Instant::now());
        sleep(Duration::from_secs(1)).await;
        println!("  [{:.1?}] Download B: DONE!", Instant::now());
        500
    });

    // Spawn task 3 — also starts immediately
    let task3 = tokio::spawn(async {
        println!("  [{:.1?}] Download C: starting (will take 2s)...", Instant::now());
        sleep(Duration::from_secs(2)).await;
        println!("  [{:.1?}] Download C: DONE!", Instant::now());
        750
    });

    // Meanwhile, the main task keeps doing its own stuff!
    println!("\n  Main: All 3 downloads started! I'll do my own work now...");
    sleep(Duration::from_millis(500)).await;
    println!("  Main: Still working... downloads are running in the background...\n");

    // Now let's wait for the results.
    // .await on a JoinHandle waits for that specific task to finish.
    // .unwrap() gets the value (it's wrapped in a Result because the task might panic)
    let size1 = task1.await.unwrap();
    let size2 = task2.await.unwrap();
    let size3 = task3.await.unwrap();

    println!("\n  All downloads complete!");
    println!("  Sizes: A={}, B={}, C={}", size1, size2, size3);
    println!("  Total bytes: {}", size1 + size2 + size3);
    println!("  Total time: {:?}  (~3s, not 6s!)", start.elapsed());

    // ===================================================
    //  DEMO 2: The `move` keyword
    // ===================================================
    //
    // What happens:
    //   Spawned tasks run on their own — they might outlive local variables.
    //   So you MUST use `move` to transfer ownership of data into the task.
    //   After `move`, you can't use that variable anymore in the main task.

    println!("\n========================================");
    println!("  Demo 2: The `move` keyword");
    println!("========================================\n");

    let name = String::from("Sujit");
    let greeting = String::from("Hello");

    // `async move` means: take ownership of `name` and `greeting`
    let handle = tokio::spawn(async move {
        // This task now OWNS `name` and `greeting`
        sleep(Duration::from_millis(100)).await;
        format!("{}, {}! Welcome to async Rust.", greeting, name)
    });

    // Can't use `name` or `greeting` here anymore — the task took them!
    // Uncommenting the next line would cause a compile error:
    // println!("{}", name);  // ERROR: value moved

    let message = handle.await.unwrap();
    println!("  {}", message);

    // ===================================================
    //  DEMO 3: Spawning many tasks in a loop
    // ===================================================
    //
    // What happens:
    //   We spawn 10 tasks in a loop. All 10 run concurrently.
    //   We collect their handles and wait for all of them.
    //   This is a very common pattern in real programs.

    println!("\n========================================");
    println!("  Demo 3: Spawning many tasks");
    println!("========================================\n");

    let start = Instant::now();
    let mut handles = Vec::new();

    for i in 1..=10 {
        // Each iteration spawns a task that runs in the background
        let handle = tokio::spawn(async move {
            // `i` is copied into the task (i32 implements Copy)
            sleep(Duration::from_millis(500)).await; // All sleep 500ms
            i * 10 // return some computed value
        });
        handles.push(handle);
    }

    println!("  Spawned 10 tasks. Waiting for all...");

    // Collect all results
    let mut results = Vec::new();
    for handle in handles {
        let value = handle.await.unwrap();
        results.push(value);
    }

    println!("  Results: {:?}", results);
    println!("  Time: {:?}  (~500ms, not 5000ms!)", start.elapsed());
    println!("  All 10 tasks ran at the same time.");
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  Demo 1:
//    - All 3 downloads START at the same time
//    - B finishes first (1s), then C (2s), then A (3s)
//    - Main task was doing its own work while downloads ran
//    - Total ~3s (the slowest download), NOT 6s (1+2+3)
//
//  Demo 2:
//    - `move` transfers ownership to the task
//    - The original variables are gone after spawning
//
//  Demo 3:
//    - 10 tasks, each sleeping 500ms, but total time is ~500ms
//    - Because they ALL slept at the same time
// =============================================================
