// =============================================================
//  Chapter 3: HOW FUTURES REALLY WORK INSIDE
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 03_how_futures_work
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  This is the deep stuff your TL Takashi wants you to understand.
//
//  Every async function becomes a FUTURE. But what IS a Future?
//  It's just a thing with one method: poll()
//
//    poll() returns either:
//      - Poll::Ready(value)  = "I'm done, here's the result"
//      - Poll::Pending       = "Not done yet, I'll tell you when I am"
//
//  HOW TOKIO USES THIS:
//  ---------------------
//    1. Tokio calls poll() on your Future
//    2. If Ready -> great, task is done
//    3. If Pending -> the Future has registered a "Waker"
//    4. When the thing it's waiting for happens, the Waker fires
//    5. Waker tells Tokio: "poll this Future again!"
//    6. Tokio polls again -> this time it might be Ready
//
//  WHAT IS A WAKER?
//  -----------------
//  A Waker is an alarm bell. When a Future returns Pending, it sets
//  an alarm. When the alarm goes off (data arrived, timer expired),
//  Tokio knows to poll the Future again.
//
//  Tokio NEVER busy-loops asking "are you done yet?". It only polls
//  when the Waker says to.
//
// =============================================================

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

// =============================================================
//  DEMO 1: Build a Future from scratch — Countdown
// =============================================================
//
//  This is what the compiler creates behind the scenes when you
//  write async fn. We're doing it by hand so you can see everything.
//
//  Our Countdown future counts down from a number to 0, then
//  returns "Liftoff!".

struct Countdown {
    count: u32,
}

// Implementing the Future trait by hand
impl Future for Countdown {
    type Output = String; // This future produces a String when done

    // This is the ONLY method a Future needs.
    // Tokio calls this repeatedly until it returns Ready.
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<String> {
        if self.count == 0 {
            // We're done! Return the final result.
            println!("    poll() called -> count=0 -> Ready!(\"Liftoff!\")");
            Poll::Ready("Liftoff!".to_string())
        } else {
            // Not done yet.
            println!("    poll() called -> count={} -> Pending", self.count);
            self.count -= 1;

            // CRITICAL: Tell Tokio to poll us again!
            // Without this line, Tokio would NEVER come back to us.
            // We'd be stuck forever returning Pending.
            cx.waker().wake_by_ref();

            Poll::Pending
        }
    }
}

// =============================================================
//  DEMO 2: A Future that actually waits — DelayedValue
// =============================================================
//
//  This is more realistic. It waits until a deadline, then returns.
//  It uses a background thread to call the Waker after the deadline.
//
//  In real Tokio, the reactor handles this (using OS-level timers),
//  not a background thread. But the principle is the same.

struct DelayedValue {
    value: String,
    deadline: Instant,
    waker_set: bool,
}

impl DelayedValue {
    fn new(value: &str, delay: Duration) -> Self {
        DelayedValue {
            value: value.to_string(),
            deadline: Instant::now() + delay,
            waker_set: false,
        }
    }
}

impl Future for DelayedValue {
    type Output = String;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<String> {
        if Instant::now() >= self.deadline {
            // Deadline passed! We're done.
            println!("    poll() -> Ready! Time's up.");
            Poll::Ready(self.value.clone())
        } else {
            println!("    poll() -> Pending. Deadline not reached yet.");

            if !self.waker_set {
                // Set up the alarm: spawn a thread that sleeps until deadline,
                // then calls the waker to tell Tokio "poll me again!"
                self.waker_set = true;
                let waker = cx.waker().clone();
                let deadline = self.deadline;

                std::thread::spawn(move || {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    println!("    [background thread] Sleeping for {:?}...", remaining);
                    std::thread::sleep(remaining);
                    println!("    [background thread] Waking the future!");
                    waker.wake(); // <- THIS tells Tokio to poll our future again
                });
            }

            Poll::Pending
        }
    }
}

#[tokio::main]
async fn main() {
    // ===================================================
    //  Running Demo 1: Countdown
    // ===================================================
    //
    // What happens:
    //   We create Countdown { count: 3 } and .await it.
    //   .await tells Tokio to call poll() on it.
    //   poll() is called 4 times:
    //     count=3 -> Pending (wake immediately)
    //     count=2 -> Pending (wake immediately)
    //     count=1 -> Pending (wake immediately)
    //     count=0 -> Ready("Liftoff!")

    println!("========================================");
    println!("  Demo 1: Countdown Future (hand-made)");
    println!("========================================\n");

    let result = Countdown { count: 3 }.await;
    println!("\n  Result: {}\n", result);

    // ===================================================
    //  Running Demo 2: DelayedValue
    // ===================================================
    //
    // What happens:
    //   We create a DelayedValue with a 2-second deadline.
    //   First poll: deadline not reached -> Pending
    //     -> spawns a background thread that sleeps 2 seconds
    //   2 seconds later: background thread calls waker.wake()
    //     -> Tokio polls again
    //   Second poll: deadline reached -> Ready!

    println!("========================================");
    println!("  Demo 2: DelayedValue Future (realistic)");
    println!("========================================\n");

    let start = Instant::now();
    let result = DelayedValue::new("Hello from the future!", Duration::from_secs(2)).await;
    println!("\n  Result: '{}' (took {:?})", result, start.elapsed());
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  Demo 1 (Countdown):
//    - poll() is called 4 times (count 3, 2, 1, 0)
//    - First 3 times: Pending (not done, count down)
//    - 4th time: Ready (count reached 0)
//    - cx.waker().wake_by_ref() caused Tokio to poll immediately
//
//  Demo 2 (DelayedValue):
//    - First poll: Pending (deadline not reached)
//    - Background thread sleeps for ~2 seconds
//    - Background thread calls waker.wake()
//    - Second poll: Ready (deadline passed)
//    - Total time: ~2 seconds
//
//  KEY TAKEAWAY:
//    A Future is polled repeatedly. It returns Pending when waiting,
//    Ready when done. The Waker tells Tokio when to poll again.
//    This is ALL that async/await does behind the scenes.
// =============================================================
