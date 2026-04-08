// =============================================================
//  Chapter 1: ASYNC FUNCTIONS AND FUTURES
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 01_async_fn_and_futures
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  This file teaches you the two most basic concepts:
//    1. async fn — a function that returns a Future (a promise of a value)
//    2. .await — the way to actually get the value from a Future
//
//  KEY IDEA:
//  ---------
//  Normal function:  you call it -> it runs -> it gives you the answer
//  Async function:   you call it -> it gives you a FUTURE (a promise)
//                    -> you .await the future -> NOW it runs -> answer
//
//  An async function is LAZY. Calling it does NOTHING until you .await it.
//
// =============================================================

use tokio::time::{sleep, Duration, Instant};

// This is an async function. It says it returns a String,
// but really it returns a Future<Output = String>.
// The String only comes out when you .await it.
async fn say_hello() -> String {
    println!("    say_hello() is actually running now!");
    "Hello, World!".to_string()
}

// This simulates a slow operation (like reading from a database)
async fn get_username() -> String {
    println!("    get_username(): Asking the database...");
    sleep(Duration::from_millis(500)).await; // Simulate 500ms database query
    println!("    get_username(): Got the answer!");
    "Sujit".to_string()
}

async fn get_age() -> u32 {
    println!("    get_age(): Asking a different database...");
    sleep(Duration::from_millis(300)).await; // Simulate 300ms query
    println!("    get_age(): Got the answer!");
    25
}

#[tokio::main]
async fn main() {
    // ===================================================
    //  DEMO 1: Calling async fn WITHOUT .await
    // ===================================================
    //
    // What happens:
    //   We call say_hello() but DON'T use .await.
    //   The function body does NOT execute.
    //   We just get a Future sitting there doing nothing.
    //
    // This is the #1 beginner mistake in Rust async.

    println!("========================================");
    println!("  Demo 1: Without .await vs With .await");
    println!("========================================\n");

    println!("  Calling say_hello() WITHOUT .await:");
    let _future = say_hello(); // <- Nothing prints! The function didn't run!
    println!("  Did you see 'say_hello is running'? NO! It didn't run.\n");

    println!("  Calling say_hello() WITH .await:");
    let result = say_hello().await; // <- NOW it actually runs!
    println!("  Result: {}\n", result);

    // ===================================================
    //  DEMO 2: Sequential .await (one after another)
    // ===================================================
    //
    // What happens:
    //   We .await get_username() first (500ms)
    //   THEN .await get_age() (300ms)
    //   Total: 500 + 300 = 800ms
    //
    // Use this when the second task DEPENDS on the first.
    // Example: you need the username before you can look up their profile.

    println!("========================================");
    println!("  Demo 2: Sequential (one after another)");
    println!("========================================\n");

    let start = Instant::now();
    let name = get_username().await; // Wait 500ms
    let age = get_age().await;       // THEN wait 300ms
    println!("  {} is {} years old", name, age);
    println!("  Time: {:?}  (500 + 300 = ~800ms)\n", start.elapsed());

    // ===================================================
    //  DEMO 3: Concurrent with tokio::join! (at the same time)
    // ===================================================
    //
    // What happens:
    //   tokio::join! starts BOTH at the same time.
    //   get_username (500ms) and get_age (300ms) run together.
    //   Total: ~500ms (the slower one)
    //
    // Use this when the tasks are INDEPENDENT (don't depend on each other).

    println!("========================================");
    println!("  Demo 3: Concurrent (at the same time)");
    println!("========================================\n");

    let start = Instant::now();
    let (name, age) = tokio::join!(get_username(), get_age()); // Both at once!
    println!("  {} is {} years old", name, age);
    println!("  Time: {:?}  (both ran together = ~500ms)", start.elapsed());
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  Demo 1:
//    - Without .await -> function didn't run (no print from inside it)
//    - With .await -> function ran and returned "Hello, World!"
//    LESSON: async fn is lazy. Always .await it.
//
//  Demo 2:
//    - get_username starts, finishes, THEN get_age starts
//    - Total ~800ms
//    LESSON: sequential .await = one after another
//
//  Demo 3:
//    - Both start at the same time (you see both "Asking" messages first)
//    - get_age finishes first (300ms), then get_username (500ms)
//    - Total ~500ms
//    LESSON: tokio::join! = concurrent = faster when tasks are independent
// =============================================================
