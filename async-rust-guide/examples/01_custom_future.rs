/// Example 1: Build a Future from scratch — no async/await, just poll().
///
/// KEY CONCEPTS:
///   - A Future is just a trait with one method: poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<T>
///   - Poll::Pending  = "not done yet, wake me later"
///   - Poll::Ready(v) = "done, here's the value"
///   - The runtime calls poll() repeatedly until Ready
///   - cx.waker() is how the future tells the runtime "I'm ready to be polled again"
///
/// RUN: cargo run --example 01_custom_future
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

// A future that counts down from N to 0, then resolves with "done!"
struct Countdown {
    remaining: u32,
}

impl Countdown {
    fn new(from: u32) -> Self {
        Countdown { remaining: from }
    }
}

impl Future for Countdown {
    type Output = &'static str;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.remaining == 0 {
            println!("  [poll] remaining=0 → returning Poll::Ready");
            Poll::Ready("done!")
        } else {
            println!("  [poll] remaining={} → returning Poll::Pending", self.remaining);
            self.remaining -= 1;

            // IMPORTANT: we must call wake() or the runtime will never poll us again.
            // In real code, you'd store the waker and call it when an I/O event fires.
            // Here we wake immediately so the runtime re-polls us right away.
            cx.waker().wake_by_ref();

            Poll::Pending
        }
    }
}

#[tokio::main]
async fn main() {
    println!("=== Custom Future: Countdown ===\n");

    // .await drives the future by calling poll() until Ready
    let result = Countdown::new(3).await;
    println!("\nResult: {result}");

    println!("\n--- What happened ---");
    println!("Tokio called poll() 4 times:");
    println!("  3 times → Pending (we called waker to re-schedule)");
    println!("  1 time  → Ready(\"done!\")");
    println!("\nThis is ALL that .await does — it calls poll() in a loop.");

    // EXERCISE: Try changing the countdown to 5. How many poll() calls do you see?
    // EXERCISE: Comment out the cx.waker().wake_by_ref() line. What happens and why?
}
