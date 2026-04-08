// =============================================================
//  Chapter 0: WHY ASYNC EXISTS — The Problem
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 00_why_async
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  This file shows you the PROBLEM that async solves.
//  We make breakfast two ways:
//    1. The SLOW way (synchronous) — do one thing, wait, then next thing
//    2. The FAST way (async) — do everything at the same time
//
//  WHAT IS TOKIO?
//  ---------------
//  Rust has "async" and ".await" keywords, but it does NOT have a built-in
//  engine to actually run async code. Tokio IS that engine.
//  Think of it like this:
//    - async/await = the recipe (what to cook)
//    - Tokio = the kitchen (where cooking actually happens)
//  Without Tokio, your async code is a recipe with no kitchen.
//
//  WHY ARE WE LEARNING THIS?
//  --------------------------
//  You're building toward a reverse proxy. A reverse proxy handles thousands
//  of network connections at the same time. Without async, you'd need one
//  thread per connection (expensive!) or everything would be painfully slow.
//  Tokio lets one thread handle thousands of connections efficiently.
//
// =============================================================

use tokio::time::{sleep, Duration, Instant};

// --- SYNCHRONOUS (BLOCKING) VERSION ---
// Each function blocks the thread. Nothing else can happen while it runs.

fn sync_make_toast() -> String {
    println!("  [Toast] Putting bread in toaster...");
    std::thread::sleep(std::time::Duration::from_secs(3)); // BLOCKS! CPU sits idle for 3 seconds
    println!("  [Toast] Done!");
    "toast".to_string()
}

fn sync_boil_water() -> String {
    println!("  [Water] Turning on kettle...");
    std::thread::sleep(std::time::Duration::from_secs(4)); // BLOCKS for 4 seconds
    println!("  [Water] Done!");
    "hot water".to_string()
}

fn sync_fry_egg() -> String {
    println!("  [Egg] Cracking egg into pan...");
    std::thread::sleep(std::time::Duration::from_secs(2)); // BLOCKS for 2 seconds
    println!("  [Egg] Done!");
    "fried egg".to_string()
}

// --- ASYNC (NON-BLOCKING) VERSION ---
// Each function uses .await which means "pause ME, let others run"

async fn async_make_toast() -> String {
    println!("  [Toast] Putting bread in toaster...");
    sleep(Duration::from_secs(3)).await; // Does NOT block! Other tasks can run!
    println!("  [Toast] Done!");
    "toast".to_string()
}

async fn async_boil_water() -> String {
    println!("  [Water] Turning on kettle...");
    sleep(Duration::from_secs(4)).await; // Does NOT block!
    println!("  [Water] Done!");
    "hot water".to_string()
}

async fn async_fry_egg() -> String {
    println!("  [Egg] Cracking egg into pan...");
    sleep(Duration::from_secs(2)).await; // Does NOT block!
    println!("  [Egg] Done!");
    "fried egg".to_string()
}

// #[tokio::main] is a shortcut that creates the Tokio runtime (the "kitchen")
// and runs our async main function on it.
#[tokio::main]
async fn main() {
    // ===================================================
    //  PART 1: The SLOW way (synchronous / blocking)
    // ===================================================
    //
    // What happens here:
    //   - make_toast runs and we WAIT 3 seconds doing nothing
    //   - THEN boil_water runs and we WAIT 4 seconds doing nothing
    //   - THEN fry_egg runs and we WAIT 2 seconds doing nothing
    //   - Total: 3 + 4 + 2 = 9 seconds
    //
    // This is like standing at the toaster staring at it,
    // THEN going to the kettle and staring at it,
    // THEN going to the pan and staring at it.
    // Nobody cooks breakfast this way!

    println!("========================================");
    println!("  SLOW WAY (synchronous / blocking)");
    println!("========================================\n");

    let start = std::time::Instant::now();

    let toast = sync_make_toast();   // Wait 3 seconds...
    let water = sync_boil_water();   // THEN wait 4 seconds...
    let egg = sync_fry_egg();        // THEN wait 2 seconds...

    println!("\n  Breakfast: {}, {}, {}", toast, water, egg);
    println!("  Total time: {:?}", start.elapsed()); // ~9 seconds!
    println!("  ^ That's slow! We waited 9 seconds.\n");

    // ===================================================
    //  PART 2: The FAST way (async / non-blocking)
    // ===================================================
    //
    // What happens here:
    //   - All three start at the same time
    //   - tokio::join! runs them concurrently
    //   - When toast hits .await, Tokio goes to check on water and egg
    //   - They all make progress simultaneously
    //   - Total: ~4 seconds (the slowest one)
    //
    // This is like putting bread in toaster, starting the kettle,
    // cracking the egg — all going at once. You check on each
    // as they finish.

    println!("========================================");
    println!("  FAST WAY (async / non-blocking)");
    println!("========================================\n");

    let start = Instant::now();

    // tokio::join! runs ALL THREE at the same time!
    let (toast, water, egg) = tokio::join!(
        async_make_toast(),
        async_boil_water(),
        async_fry_egg()
    );

    println!("\n  Breakfast: {}, {}, {}", toast, water, egg);
    println!("  Total time: {:?}", start.elapsed()); // ~4 seconds!
    println!("  ^ Much faster! All three ran at the same time.");
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  SLOW WAY output:
//    [Toast] starting -> [Toast] done (3s) ->
//    [Water] starting -> [Water] done (4s) ->
//    [Egg] starting -> [Egg] done (2s)
//    Total: ~9 seconds (one after another)
//
//  FAST WAY output:
//    [Toast] starting, [Water] starting, [Egg] starting (ALL AT ONCE!)
//    [Egg] done (2s) -> [Toast] done (3s) -> [Water] done (4s)
//    Total: ~4 seconds (all ran together)
//
//  The difference: 9 seconds vs 4 seconds. Same work, less waiting.
//  That's the whole point of async.
// =============================================================
