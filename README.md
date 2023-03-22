# Rust Async Timer

A Rust async timer, purely software implementation, fires at milliseconds level accuracy, check the unit test case for how this is measured.  
  
An **AsyncTimer** object allows application to arm and disarm thousands of timers, and they all share same async runner task in it.  
Each timer accepts an unique Rust async block (a future) as callback, when timer expires, this async block will be executed by the **AsyncTimer** object's async runner task.  
An application can use this timer from any asynchronous context freely.

```
let timer = AsyncTimer::new().await?;

// Arm a timer that will expire in 5s
let on_expire = FutureObject::new(async move {
    println!("Timer 5s expired!");
    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
});
// The returned handle 't1' can be used to disarm it later
let t1 = timer.add(5000, Some(on_expire)).await?;

// Maybe disarm it later?
// timer.delete(t1).await?;

```
All active timers are on a priority queue in that runner task, so if one timer's async block takes long time to finish, the timers behind it has no chance to execute, this is the known limitation of this solution, so use it when it suit your use case.
  
Generally speaking, a "callback" style code is anti-pattern in Rust async/await code, but I found this useful when I want an async block to be able to retry an UDP request on response timeout, if I just await on the response to timeout, then that task can not handle next request. Now I can use a timer to do the retry on timeout, and the async block can go back to handle next request, without need for spawning a new task for each request, which may be costly.  
The timer based retry solution, also decouples the sending request and waiting for response, so a requester can send out an array of requesters to a remote responder, and the responder can handle them in an arbitrary order, thus achieves stateless between the two parts.  

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
