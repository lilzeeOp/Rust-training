/// Example 2: Pin and why it exists — the self-referential struct problem.
///
/// KEY CONCEPTS:
///   - async fn compiles into a state machine struct (a Future)
///   - Local variables alive across .await become fields in that struct
///   - If a field is a reference to another field → self-referential struct
///   - Moving a self-referential struct invalidates internal pointers → UB
///   - Pin<&mut T> guarantees the value won't be moved → safe to have self-references
///   - This is why Future::poll takes Pin<&mut Self>, not &mut self
///
/// RUN: cargo run --example 02_pin_demo
use std::pin::Pin;

fn main() {
    println!("=== Pin: Why It Exists ===\n");

    // --- Part 1: The problem with moving ---
    demo_move_problem();

    // --- Part 2: What async fn generates ---
    demo_async_state_machine();

    // --- Part 3: Pin in action ---
    demo_pin_basics();
}

fn demo_move_problem() {
    println!("--- Part 1: The Move Problem ---\n");

    // Imagine this struct (pseudocode — we can't actually build a safe self-ref in Rust):
    //
    //   struct SelfRef {
    //       data: String,
    //       ptr: *const String,  // points to self.data
    //   }
    //
    // If you move SelfRef to a new memory location:
    //   - data moves to a new address
    //   - ptr still points to the OLD address → dangling pointer!

    let data = String::from("hello");
    let ptr = &data as *const String;

    println!("  Before move: data is at {:p}", &data);
    println!("  ptr points to:          {:p}", ptr);

    let moved_data = data; // data moves to moved_data
    println!("  After move:  data is at {:p}", &moved_data);
    println!("  ptr STILL points to:    {:p} ← DANGLING!", ptr);
    println!();
}

fn demo_async_state_machine() {
    println!("--- Part 2: What async fn Compiles To ---\n");

    // This async fn:
    //
    //   async fn example(data: &mut Vec<u8>) {
    //       let slice = &data[..];    // borrows data
    //       tokio::time::sleep(...).await;  // suspension point!
    //       println!("{:?}", slice);   // uses borrow after .await
    //   }
    //
    // Compiles roughly to:
    //
    //   enum ExampleFuture<'a> {
    //       State0 { data: &'a mut Vec<u8> },                    // before .await
    //       State1 { data: &'a mut Vec<u8>, slice: &'a [u8] },   // across .await
    //       //                              ^^^^^^^^^^^^^^^^
    //       //                              slice points into data — SELF-REFERENTIAL!
    //       Done,
    //   }
    //
    // If this struct is moved while in State1, slice becomes a dangling pointer.
    // Pin prevents this move.

    println!("  async fn with a borrow across .await creates a self-referential future.");
    println!("  Pin<&mut Self> in poll() guarantees the future won't move after first poll.");
    println!("  That's why the borrow checker forbids &mut across .await in some cases.\n");
}

fn demo_pin_basics() {
    println!("--- Part 3: Pin Basics ---\n");

    // Pin<&mut T> wraps a mutable reference and removes the ability to move T.

    let mut value = 42u32;

    // Pin a value to its current location
    // SAFETY: u32 implements Unpin, so this is always safe.
    // For types that are !Unpin (like most futures), you'd use Box::pin().
    let pinned: Pin<&mut u32> = Pin::new(&mut value);

    // For Unpin types, you can still get &mut — Pin is a no-op for them
    let inner: &mut u32 = Pin::into_inner(pinned);
    *inner = 99;
    println!("  Unpin type (u32): Pin lets you get &mut back — no restriction.");
    println!("  value = {}\n", value);

    // For !Unpin types (futures), Pin prevents you from getting &mut,
    // so you can't call std::mem::swap or std::mem::replace to move them.
    println!("  !Unpin type (most futures): Pin blocks &mut access.");
    println!("  → Can't mem::swap → Can't move → Self-references stay valid.\n");

    // Box::pin is the common way to pin a future on the heap:
    let future = Box::pin(async {
        // This future is pinned — it will never be moved after creation.
        // Internal self-references (if any) are safe.
        "pinned future result"
    });

    println!("  Box::pin(async {{ ... }}) → Pin<Box<dyn Future>>  (heap-pinned)");
    println!("  tokio::pin!(future)       → Pin<&mut impl Future> (stack-pinned)\n");

    // We need to actually use the future to avoid a warning
    drop(future);

    println!("  EXERCISE: Read the compiler error when you try:");
    println!("    let mut f = async {{ tokio::time::sleep(...).await; }};");
    println!("    let p = Pin::new(&mut f);  // ERROR: async block is !Unpin");
    println!("    // Fix: use Box::pin(f) or tokio::pin!(f) instead\n");
}
