// =============================================================
//  Chapter 7: COMMON MISTAKES That Will Bite You
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 07_common_mistakes
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  Every async Rust beginner makes these mistakes. This file shows
//  you each mistake and the correct way to do it, so you don't
//  have to learn the hard way.
//
//  MISTAKES COVERED:
//    1. Blocking the runtime (using std::thread::sleep in async)
//    2. Forgetting .await (future does nothing)
//    3. Holding std::sync::Mutex across .await (deadlock risk)
//
// =============================================================

use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};

#[tokio::main]
async fn main() {
    // ===================================================
    //  MISTAKE 1: Blocking the runtime
    // ===================================================
    //
    // What happens:
    //   We use a SINGLE-THREADED runtime on purpose so you can
    //   clearly see the problem.
    //
    //   BAD: std::thread::sleep BLOCKS the thread.
    //     Task A blocks for 1 second. Task B can't even START
    //     until A's sleep is over. Total: 2 seconds.
    //
    //   GOOD: tokio::time::sleep YIELDS to the runtime.
    //     Task A hits .await and Tokio runs Task B.
    //     Both sleep at the same time. Total: 1 second.

    println!("========================================");
    println!("  Mistake 1: Blocking the runtime");
    println!("========================================\n");

    // BAD way
    println!("  --- BAD: std::thread::sleep (BLOCKS!) ---");
    let start = Instant::now();

    let a = tokio::spawn(async {
        println!("  Task A: starting");
        std::thread::sleep(std::time::Duration::from_secs(1)); // BLOCKS!
        println!("  Task A: done");
    });
    let b = tokio::spawn(async {
        println!("  Task B: starting");
        std::thread::sleep(std::time::Duration::from_secs(1)); // BLOCKS!
        println!("  Task B: done");
    });
    a.await.unwrap();
    b.await.unwrap();
    println!("  Time: {:?}  (should be ~1s but is ~2s because of blocking!)\n", start.elapsed());

    // GOOD way
    println!("  --- GOOD: tokio::time::sleep (yields!) ---");
    let start = Instant::now();

    let a = tokio::spawn(async {
        println!("  Task A: starting");
        sleep(Duration::from_secs(1)).await; // Yields! Other tasks can run!
        println!("  Task A: done");
    });
    let b = tokio::spawn(async {
        println!("  Task B: starting");
        sleep(Duration::from_secs(1)).await; // Yields!
        println!("  Task B: done");
    });
    a.await.unwrap();
    b.await.unwrap();
    println!("  Time: {:?}  (both ran at the same time!)\n", start.elapsed());

    println!("  RULE: Never use these in async code:");
    println!("    std::thread::sleep  -> use tokio::time::sleep");
    println!("    std::fs::read       -> use tokio::fs::read");
    println!("    Any blocking I/O    -> use tokio async version\n");
    println!("  If you MUST use blocking code, wrap it in:");
    println!("    tokio::task::spawn_blocking(|| {{ ... }})\n");

    // ===================================================
    //  MISTAKE 2: Forgetting .await
    // ===================================================
    //
    // What happens:
    //   Calling an async function WITHOUT .await creates a Future
    //   but NEVER runs it. The code inside doesn't execute.
    //   The compiler warns you about this, but beginners ignore warnings.

    println!("========================================");
    println!("  Mistake 2: Forgetting .await");
    println!("========================================\n");

    async fn important_work() {
        println!("  >> This line proves the function actually ran!");
    }

    println!("  Calling important_work() WITHOUT .await:");
    let _unused_future = important_work(); // Does NOTHING!
    println!("  Did the function run? NO! (no print from inside)\n");

    println!("  Calling important_work() WITH .await:");
    important_work().await; // NOW it runs!
    println!("  Now it ran!\n");

    println!("  RULE: The Rust compiler warns you about unused futures.");
    println!("  ALWAYS read compiler warnings!\n");

    // ===================================================
    //  MISTAKE 3: Holding a mutex lock across .await
    // ===================================================
    //
    // What happens:
    //   If you lock a std::sync::Mutex and then .await while holding it,
    //   the lock is held the ENTIRE time you're paused.
    //   Other tasks waiting for the lock are BLOCKED.
    //   This can cause deadlocks (program freezes forever).
    //
    //   Solution: Use tokio::sync::Mutex if you need to hold it across .await.
    //   Or: lock, do quick work, drop lock BEFORE the .await.

    println!("========================================");
    println!("  Mistake 3: Mutex across .await");
    println!("========================================\n");

    // GOOD pattern: lock briefly, drop before .await
    println!("  --- GOOD: Lock briefly, drop before .await ---");
    let data = Arc::new(std::sync::Mutex::new(Vec::new()));

    let data_clone = data.clone();
    let t1 = tokio::spawn(async move {
        // Lock, do quick work, then drop lock BEFORE sleeping
        {
            let mut lock = data_clone.lock().unwrap();
            lock.push(1);
            println!("  Task 1: pushed 1, releasing lock");
        } // lock is dropped HERE, before the .await below
        sleep(Duration::from_millis(100)).await; // Safe! Lock is already released.
    });

    let data_clone = data.clone();
    let t2 = tokio::spawn(async move {
        sleep(Duration::from_millis(50)).await; // Small delay to ensure order
        {
            let mut lock = data_clone.lock().unwrap();
            lock.push(2);
            println!("  Task 2: pushed 2, releasing lock");
        }
    });

    t1.await.unwrap();
    t2.await.unwrap();
    println!("  Data: {:?}\n", data.lock().unwrap());

    // ALSO GOOD: Use tokio::sync::Mutex if you NEED to hold across .await
    println!("  --- ALSO GOOD: tokio::sync::Mutex (async-aware) ---");
    let data = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let d1 = data.clone();
    let t1 = tokio::spawn(async move {
        let mut lock = d1.lock().await; // This .await is fine!
        println!("  Task 1: got async lock, working...");
        sleep(Duration::from_millis(100)).await; // Safe to hold across .await!
        lock.push(1);
        println!("  Task 1: done, releasing lock");
    });

    let d2 = data.clone();
    let t2 = tokio::spawn(async move {
        let mut lock = d2.lock().await; // Will wait until task 1 releases
        println!("  Task 2: got async lock, working...");
        lock.push(2);
        println!("  Task 2: done, releasing lock");
    });

    t1.await.unwrap();
    t2.await.unwrap();
    println!("  Data: {:?}", data.lock().await);

    println!("\n  RULE:");
    println!("    - Quick lock, no .await while holding -> std::sync::Mutex is fine");
    println!("    - Need to .await while holding -> use tokio::sync::Mutex");
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  Mistake 1:
//    BAD: ~2 seconds (tasks ran one after another because blocking)
//    GOOD: ~1 second (tasks ran concurrently)
//
//  Mistake 2:
//    Without .await: the function body never executed
//    With .await: it ran and we saw the print
//
//  Mistake 3:
//    Brief lock pattern: lock, push, drop, THEN await (safe)
//    tokio::sync::Mutex: can hold across .await (also safe)
//
//  SUMMARY OF RULES:
//    1. Never use blocking functions in async code
//    2. Always .await your futures (read compiler warnings!)
//    3. Don't hold std::sync::Mutex across .await points
// =============================================================
