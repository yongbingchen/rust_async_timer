# Rust Async Timer

A Rust async timer, purely software implementation, fires at milliseconds accuracy level, check the unit test case for example.  
  
An **AsyncTimer** object allows application to arm and disarm thousands of timers, and they all share same async task runner context in it.  
Each timer accepts an unique Rust async block (a future) as callback, when timer expires, this async block will be executed by the **AsyncTimer** object's async task runner.  
This allows application to use it from any asynchronous context freely.

```
let timer = AsyncTimer::new().await?;

// Add an item that will expire in 5s
let on_expire = FutureObject::new(async move {
    println!("Timer 5s expired!");
    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
});
let handle = timer.add(5000, Some(on_expire)).await?;
```

Run with "cargo run --release" to see the sample application without debug log.

```
$ cargo run --release
   Compiling async_sw_timer v0.1.0 (~/workspace/rust_async_timer)
    Finished release [optimized] target(s) in 3.82s
     Running `target/release/async_sw_timer`
Timer 1s expired!
Timer 2s expired!
Timer 4s expired!
Timer 5s expired!
main exiting, now test drop the timer
```
