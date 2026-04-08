/// Example 3: Tokio runtime — spawning tasks, how the scheduler works.
///
/// KEY CONCEPTS:
///   - #[tokio::main] creates a multi-threaded runtime with a work-stealing scheduler
///   - tokio::spawn() moves a future onto the runtime as a new task
///   - Tasks are like green threads — many tasks run on few OS threads
///   - spawn() requires 'static + Send (the future must own its data and be thread-safe)
///   - JoinHandle lets you await a spawned task's result
///
/// RUN: cargo run --example 03_tokio_spawn
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() {
    println!("=== Tokio Runtime & Spawning ===\n");

    // --- Part 1: Sequential vs Concurrent ---
    part1_sequential_vs_concurrent().await;

    // --- Part 2: spawn requires 'static + Send ---
    part2_spawn_constraints().await;

    // --- Part 3: JoinHandle and error propagation ---
    part3_join_handle().await;
}

async fn part1_sequential_vs_concurrent() {
    println!("--- Part 1: Sequential vs Concurrent ---\n");

    // Sequential: total time = 200ms + 200ms = ~400ms
    let start = std::time::Instant::now();
    let a = do_work("A-seq", 200).await;
    let b = do_work("B-seq", 200).await;
    println!("  Sequential: {a} + {b} took {:?}\n", start.elapsed());

    // Concurrent with spawn: total time = max(200ms, 200ms) = ~200ms
    let start = std::time::Instant::now();
    let handle_a = tokio::spawn(do_work("A-conc", 200));
    let handle_b = tokio::spawn(do_work("B-conc", 200));
    let a = handle_a.await.unwrap();
    let b = handle_b.await.unwrap();
    println!("  Concurrent: {a} + {b} took {:?}\n", start.elapsed());

    // Concurrent with join!: same idea, often more convenient
    let start = std::time::Instant::now();
    let (a, b) = tokio::join!(do_work("A-join", 200), do_work("B-join", 200));
    println!("  Join macro: {a} + {b} took {:?}\n", start.elapsed());
}

async fn part2_spawn_constraints() {
    println!("--- Part 2: spawn requires 'static + Send ---\n");

    // This works — String is 'static + Send
    let owned = String::from("owned data");
    let handle = tokio::spawn(async move {
        // `owned` was MOVED into this future — it's now 'static
        println!("  Task has: {owned}");
    });
    handle.await.unwrap();

    // This would NOT compile:
    //   let local = String::from("borrowed");
    //   tokio::spawn(async {
    //       println!("{}", &local);  // ERROR: borrows local, not 'static
    //   });
    //
    // Why? The spawned task might outlive the current scope.
    // Tokio could schedule it on a different thread after this function returns.
    // Fix: use `move` to transfer ownership, or clone.

    // This would NOT compile either:
    //   let rc = std::rc::Rc::new(42);
    //   tokio::spawn(async move {
    //       println!("{rc}");  // ERROR: Rc is not Send
    //   });
    //
    // Why? Rc uses non-atomic reference counting — not thread-safe.
    // Fix: use Arc instead.

    println!("  spawn() needs 'static: future must own all its data (use `move`)");
    println!("  spawn() needs Send: data must be safe to send across threads (use Arc, not Rc)\n");
}

async fn part3_join_handle() {
    println!("--- Part 3: JoinHandle ---\n");

    // Normal completion
    let handle = tokio::spawn(async { 42 });
    match handle.await {
        Ok(value) => println!("  Task returned: {value}"),
        Err(e) => println!("  Task panicked: {e}"),
    }

    // Panic in spawned task — doesn't crash the runtime!
    let handle = tokio::spawn(async {
        panic!("oops");
    });
    match handle.await {
        Ok(_) => println!("  Task succeeded"),
        Err(e) => println!("  Task panicked (caught): {e}"),
    }

    // Abort a task
    let handle = tokio::spawn(async {
        sleep(Duration::from_secs(60)).await;
        "never reached"
    });
    handle.abort();
    match handle.await {
        Ok(v) => println!("  Got: {v}"),
        Err(e) if e.is_cancelled() => println!("  Task was aborted: {e}"),
        Err(e) => println!("  Task error: {e}"),
    }

    println!();
    println!("  EXERCISE: Spawn 10 tasks that each sleep for a random duration.");
    println!("  Collect all results with JoinSet. Which finishes first?\n");
}

async fn do_work(name: &'static str, ms: u64) -> String {
    sleep(Duration::from_millis(ms)).await;
    format!("{name}({ms}ms)")
}
