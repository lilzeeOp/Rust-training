// =============================================================
//  Chapter 4: STATE MACHINES — What The Compiler Does
// =============================================================
//
//  Run this file:
//      cd async-rust-guide
//      cargo run --example 04_state_machines
//
//  WHAT IS THIS FILE ABOUT?
//  -------------------------
//  When you write async fn, the Rust compiler transforms your code
//  into a STATE MACHINE. This file shows you exactly what that means.
//
//  WHAT IS A STATE MACHINE?
//  -------------------------
//  A state machine is a thing that:
//    1. Has a current state (like "Step 1", "Step 2", "Done")
//    2. Transitions between states based on what happens
//    3. Remembers data from previous states
//
//  YOUR ASYNC FUNCTION AS A STATE MACHINE:
//  ----------------------------------------
//  async fn do_stuff() -> String {
//      let a = step_one().await;    // <-- await point 1
//      let b = step_two(a).await;   // <-- await point 2
//      format!("{} -> {}", a, b)
//  }
//
//  Becomes:
//    State 0: NotStarted (haven't begun)
//    State 1: WaitingForStepOne (called step_one, waiting for result)
//    State 2: WaitingForStepTwo (got a, called step_two, waiting)
//    State 3: Done (got b, computed the final string)
//
//  Each .await point = a place where the state machine can PAUSE
//  and let other tasks run.
//
//  WHY THIS MATTERS:
//  ------------------
//    - Zero cost: The state machine is a fixed-size struct, no heap allocation
//    - No garbage collector needed
//    - The compiler does this FOR YOU when you write async fn
//    - You never write state machines by hand (this file is just for learning)
//
// =============================================================

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

// =============================================================
//  The state machine, written by hand
// =============================================================
//
//  This is what the compiler generates for:
//
//  async fn two_step_job() -> String {
//      let a = step_one().await;
//      let b = step_two(a).await;
//      format!("{} then {}", a, b)
//  }

// All possible states
enum State {
    NotStarted,
    WaitingStep1 { polls_remaining: u32 },
    WaitingStep2 { step1_result: String, polls_remaining: u32 },
    Completed,
}

struct TwoStepJob {
    state: State,
}

impl TwoStepJob {
    fn new() -> Self {
        TwoStepJob {
            state: State::NotStarted,
        }
    }
}

impl Future for TwoStepJob {
    type Output = String;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<String> {
        loop {
            match &mut self.state {
                // ---- State: Haven't started yet ----
                State::NotStarted => {
                    println!("    [NotStarted] Starting step 1...");
                    // Transition to WaitingStep1
                    // (In real code, this would call the inner async fn)
                    self.state = State::WaitingStep1 { polls_remaining: 2 };
                    // Don't return — continue the loop to poll the next state
                }

                // ---- State: Waiting for step 1 to complete ----
                State::WaitingStep1 { polls_remaining } => {
                    if *polls_remaining == 0 {
                        // Step 1 is done! Save the result, move to step 2.
                        println!("    [WaitingStep1] Step 1 complete! Result: \"data-from-step-1\"");
                        let result = "data-from-step-1".to_string();
                        self.state = State::WaitingStep2 {
                            step1_result: result,
                            polls_remaining: 2,
                        };
                        // Continue loop to start step 2 immediately
                    } else {
                        // Step 1 not done yet. Return Pending.
                        println!(
                            "    [WaitingStep1] Not ready yet ({} polls left). Returning Pending.",
                            polls_remaining
                        );
                        *polls_remaining -= 1;
                        cx.waker().wake_by_ref(); // Ask to be polled again
                        return Poll::Pending;
                    }
                }

                // ---- State: Waiting for step 2 to complete ----
                // Notice: we still have step1_result stored in this state!
                State::WaitingStep2 {
                    step1_result,
                    polls_remaining,
                } => {
                    if *polls_remaining == 0 {
                        // Step 2 is done! Compute the final result.
                        println!("    [WaitingStep2] Step 2 complete! Result: \"data-from-step-2\"");
                        let final_result =
                            format!("{} then data-from-step-2", step1_result);
                        self.state = State::Completed;
                        return Poll::Ready(final_result);
                    } else {
                        println!(
                            "    [WaitingStep2] Not ready yet ({} polls left). Returning Pending.",
                            polls_remaining
                        );
                        *polls_remaining -= 1;
                        cx.waker().wake_by_ref();
                        return Poll::Pending;
                    }
                }

                // ---- State: Already completed ----
                State::Completed => {
                    panic!("Bug! Polled a future that already completed.");
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    println!("========================================");
    println!("  State Machine: Watch the transitions");
    println!("========================================\n");

    let result = TwoStepJob::new().await;
    println!("\n  Final result: {}", result);

    println!("\n========================================");
    println!("  Now the same thing with real async fn");
    println!("========================================\n");

    // This is the SAME logic, but written as a normal async fn.
    // The compiler generates the state machine for you!
    let result = two_step_job_real().await;
    println!("  Final result: {}", result);

    println!("\n  Both produce the same result.");
    println!("  The compiler turns the async fn into the state machine.");
    println!("  You never need to write state machines by hand!");
}

// This is the "normal" way — the compiler turns this into a state machine
async fn step_one() -> String {
    println!("    step_one: working...");
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    println!("    step_one: done!");
    "data-from-step-1".to_string()
}

async fn step_two(input: &str) -> String {
    println!("    step_two: working with '{}'...", input);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    println!("    step_two: done!");
    format!("{} then data-from-step-2", input)
}

async fn two_step_job_real() -> String {
    let a = step_one().await;       // Compiler creates state transition here
    let b = step_two(&a).await;     // And here
    b                               // Final result
}

// =============================================================
//  WHAT TO NOTICE WHEN YOU RUN THIS:
//
//  Hand-made state machine:
//    [NotStarted] -> [WaitingStep1] polls 3 times -> [WaitingStep2] polls 3 times -> Ready
//    You can see every single state transition and poll.
//
//  Real async fn:
//    Does the exact same thing, but the code is clean and simple.
//    The compiler generated the ugly state machine for you.
//
//  KEY TAKEAWAY:
//    - Every async fn becomes a state machine struct
//    - Each .await = a state where the machine can pause
//    - Data is stored in the struct between pauses
//    - This is zero-cost: no heap allocation, no garbage collector
//    - You get the clean syntax, the compiler does the hard work
// =============================================================
