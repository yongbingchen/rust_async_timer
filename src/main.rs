#![feature(binary_heap_retain)]
mod async_sw_timer;
use async_sw_timer::AsyncTimer;
use async_sw_timer::FutureObject;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    {
        // This parenthesis is for testing AsyncTimer's Drop trait.
        let timer = AsyncTimer::new().await?;

        // Add an item that will expire in 5s
        let on_expire = FutureObject::new(async move {
            println!("Timer 5s expired!");
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        });
        let _handle = timer.add(5000, Some(on_expire)).await?;

        // Add an item that will expire in 1s
        let on_expire = FutureObject::new(async move {
            println!("Timer 1s expired!");
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        });
        let _handle = timer.add(1000, Some(on_expire)).await?;

        // Add an item that will expire in 4s
        let on_expire = FutureObject::new(async move {
            println!("Timer 4s expired!");
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        });
        let _handle = timer.add(4000, Some(on_expire)).await?;

        // Add an item that will expire in 2s
        let on_expire = FutureObject::new(async move {
            println!("Timer 2s expired!");
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        });
        let _handle = timer.add(2000, Some(on_expire)).await?;

        // Add an item that will expire in 3s
        let on_expire = FutureObject::new(async move {
            println!("Timer 3s expired!");
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        });
        let handle3 = timer.add(3000, Some(on_expire)).await?;

        // delete an item
        timer.delete(handle3).await?;

        // wait for any remaining items to expire
        tokio::time::sleep(tokio::time::Duration::from_millis(6000)).await;
    }

    println!("main exiting, now test drop the timer");
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    Ok(())
}
