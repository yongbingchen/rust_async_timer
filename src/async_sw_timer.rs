use futures::lock::Mutex;
use std::collections::BinaryHeap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::time::{self, Duration};

// Add Sync trait for this_future, is for fixing "future cannot be sent between threads safely  the trait `Sync` is not implemented for `(dyn futures::Future<Output = ()> + std::marker::Send + 'static)`"
// The other fix is to use a mutex: this_future: Mutex<Pin<Box<dyn Future<Output = ()> + Send>>>,
pub struct FutureObject<T> {
    this_future: Pin<Box<dyn Future<Output = T> + Send + Sync>>,
}

impl<T> FutureObject<T> {
    pub fn new(future: impl Future<Output = T> + Send + Sync + 'static) -> Self {
        FutureObject {
            this_future: Box::pin(future),
        }
    }
}

// Debug trait is required by tx.send(a_future).await.unwrap();
impl<T> fmt::Debug for FutureObject<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FutureObject")
            .field("this_future", &"Future<Output=T>")
            .finish()
    }
}

// cargo run, default debug version will show debug message
#[cfg(debug_assertions)]
macro_rules! debug_println {
    ($($x:tt)*) => { println!($($x)*) }
}

// cargo run --release, release version will not show debug message
#[cfg(not(debug_assertions))]
macro_rules! debug_println {
    ($($x:tt)*) => {{}};
}

pub type OnExpire = FutureObject<()>;

struct Expiration {
    handle: u64,
    item: Option<OnExpire>,
    deadline: time::Instant,
    cancel: bool,
}

impl Expiration {
    fn new(handle: u64, item: Option<OnExpire>, deadline: time::Instant, cancel: bool) -> Self {
        Expiration {
            handle,
            item,
            deadline,
            cancel,
        }
    }
}

impl PartialEq for Expiration {
    fn eq(&self, other: &Self) -> bool {
        self.deadline.eq(&other.deadline)
    }
}

impl Eq for Expiration {}

impl PartialOrd for Expiration {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // self.deadline.partial_cmp(&other.deadline) // This makes the queue in descending order
        // This makes the queue in ascending order
        other.deadline.partial_cmp(&self.deadline)
    }
}

impl Ord for Expiration {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // self.deadline.cmp(&other.deadline) // This makes the queue in descending order
        other.deadline.cmp(&self.deadline)
    }
}

// This is required by expirations_tx.try_send()
impl fmt::Debug for Expiration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Expiration")
            .field("handle", &self.handle)
            .field("item", &self.item.as_ref().map(|_| "..."))
            .field("deadline", &self.deadline)
            .field("cancel", &self.cancel)
            .finish()
    }
}

struct Inner {
    expirations: Arc<Mutex<BinaryHeap<Expiration>>>,
    expirations_tx: mpsc::Sender<Expiration>,
    expirations_rx: Arc<Mutex<mpsc::Receiver<Expiration>>>,
    wakeup_tx: mpsc::Sender<()>,
    wakeup_rx: Arc<Mutex<mpsc::Receiver<()>>>,
    id: Arc<AtomicU64>,
    exit: Arc<AtomicBool>,
    started: Arc<AtomicU64>,
}

pub struct AsyncTimer {
    inner: Inner,
}

async fn wakeable_sleep(deadline: time::Instant, wakeup: Arc<Mutex<Receiver<()>>>) {
    let delay = tokio::time::sleep_until(deadline);
    tokio::pin!(delay);
    let mut wakeup = wakeup.lock().await;

    loop {
        tokio::select! {
            () = &mut delay => {
                debug_println!("slept to the end");
                break;
            }
            _ = wakeup.recv() => {
                // Cancel the delay by drop it.
                debug_println!("someone called wake up, abort sleep");
                drop(delay);
                break;
            }
        }
    }
}

impl Inner {
    fn new() -> Self {
        let (wakeup_tx, wakeup_rx) = mpsc::channel(1);
        let (expirations_tx, expirations_rx) = mpsc::channel::<Expiration>(100);
        Inner {
            expirations_tx,
            expirations_rx: Arc::new(Mutex::new(expirations_rx)),
            wakeup_tx,
            wakeup_rx: Arc::new(Mutex::new(wakeup_rx)),
            id: Arc::new(AtomicU64::new(0)),
            exit: Arc::new(AtomicBool::new(false)),
            started: Arc::new(AtomicU64::new(0)),
            expirations: Arc::new(Mutex::new(BinaryHeap::new())),
        }
    }

    async fn start_timer_task(&self) -> Result<(), Box<dyn std::error::Error>> {
        let exit = Arc::clone(&self.exit);
        let started = Arc::clone(&self.started);
        let wakeup_rx = Arc::clone(&self.wakeup_rx);
        let expirations = Arc::clone(&self.expirations);
        tokio::spawn(async move {
            started.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            loop {
                {
                    let expirations = expirations.lock().await;
                    let deadline = expirations
                        .peek()
                        .map(|e| e.deadline)
                        .unwrap_or_else(|| time::Instant::now() + Duration::new(1_000_000_000, 0));
                    debug_println!(
                        "current {:?}, next deadline {:?}",
                        time::Instant::now(),
                        deadline
                    );
                    for _x in expirations.iter() {
                        debug_println!("{:?}", _x.deadline);
                    }
                    drop(expirations);
                    let wakeup_rx = wakeup_rx.clone();
                    wakeable_sleep(deadline, wakeup_rx).await;
                }

                if exit.load(std::sync::atomic::Ordering::Relaxed) {
                    debug_println!("Timer runner task exiting");
                    break;
                }
                let mut expirations = expirations.lock().await;
                while let Some(_) = expirations
                    .peek()
                    .filter(|e| e.deadline <= time::Instant::now())
                {
                    let expiration = expirations.pop().unwrap();
                    if let Some(on_expire) = expiration.item {
                        debug_println!("call callback of timer {:?}", expiration.deadline);
                        on_expire.this_future.await;
                    }
                }
            }
        });
        Ok(())
    }

    async fn start_recv_task(&self) -> Result<(), Box<dyn std::error::Error>> {
        let started = Arc::clone(&self.started);
        let expirations = Arc::clone(&self.expirations);
        let expirations_rx = Arc::clone(&self.expirations_rx);
        let wakeup_tx = self.wakeup_tx.clone();
        let exit = Arc::clone(&self.exit);
        tokio::spawn(async move {
            started.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let mut expirations_rx = expirations_rx.lock().await;
            loop {
                if let Some(expiration) = expirations_rx.recv().await {
                    {
                        let mut expirations = expirations.lock().await;
                        if expiration.cancel {
                            expirations.retain(|item| item.handle != expiration.handle);
                        } else {
                            debug_println!("push {:?} to queue", expiration.deadline);
                            expirations.push(expiration);
                        }
                    }
                    debug_println!("wake up sleeping branch");
                    wakeup_tx.send(()).await.unwrap();
                }
                if exit.load(std::sync::atomic::Ordering::Relaxed) {
                    debug_println!("Timer receiving task exiting");
                    break;
                }
            }
        });

        Ok(())
    }

    async fn wait_for_task_start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let started = Arc::clone(&self.started);
        while 2 != started.load(std::sync::atomic::Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        Ok(())
    }

    fn stop(&self) {
        self.exit.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl AsyncTimer {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let inner = Inner::new();
        inner.start_timer_task().await?;
        inner.start_recv_task().await?;
        inner.wait_for_task_start().await?;
        Ok(AsyncTimer { inner })
    }

    pub async fn add(
        &self,
        timeout_ms: u64,
        on_expire: Option<OnExpire>,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let id = Arc::clone(&self.inner.id);
        let handle = id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let deadline = time::Instant::now() + Duration::from_millis(timeout_ms);
        let expiration = Expiration::new(handle, on_expire, deadline, false);
        let expirations_tx = self.inner.expirations_tx.clone();
        debug_println!("add timer {:?}", expiration.deadline);
        expirations_tx
            .send(expiration)
            .await
            .map_err(|_e| format!("send expiration failed"))?;
        Ok(handle)
    }

    pub async fn delete(&self, handle: u64) -> Result<(), Box<dyn std::error::Error>> {
        let expirations_tx = self.inner.expirations_tx.clone();
        let expiration = Expiration::new(
            handle,
            None,
            time::Instant::now() + Duration::new(1_000_000_000, 0),
            true,
        );
        expirations_tx
            .send(expiration)
            .await
            .map_err(|_e| format!("send cancel expiration failed"))?;
        Ok(())
    }
}

// Async Drop is hard to do: https://stackoverflow.com/questions/71541765/rust-async-drop
impl Drop for AsyncTimer {
    fn drop(&mut self) {
        self.inner.stop();
        let expirations_tx = self.inner.expirations_tx.clone();
        let expiration = Expiration::new(
            0,
            None,
            time::Instant::now() + Duration::new(1_000_000_000, 0),
            true,
        );
        expirations_tx.try_send(expiration).unwrap();
    }
}

#[tokio::test]
async fn test_async_timer() -> Result<(), Box<dyn std::error::Error>> {
    let timer = AsyncTimer::new().await?;

    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    let seed: [u8; 32] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
        26, 27, 28, 29, 30, 31, 32,
    ];
    let mut rng = StdRng::from_seed(seed);

    let mut deleted = Vec::new();
    let pendings = Arc::new(Mutex::new(Vec::new()));
    for _ in 1..100 {
        let now = time::Instant::now();
        // Plus 1s to let the delete operation done
        let delay = rng.gen::<u64>() % 10000 + 1000;
        let pendings_c = pendings.clone();

        let on_expire = FutureObject::new(async move {
            let deadline = now + Duration::from_millis(delay);
            let real_time = time::Instant::now();
            let mut pendings = pendings_c.lock().await;
            pendings
                .iter_mut()
                .find(|(_, d, _)| d == &delay)
                .map(|(_, _, flag)| *flag = true);
            assert!(
                real_time > deadline && real_time < deadline + Duration::from_millis(5),
                "Timer fired at wrong time"
            );
        });

        let h = timer.add(delay, Some(on_expire)).await?;
        let mut pendings = pendings.lock().await;
        pendings.push((h, delay, false));
    }

    {
        let pendings = pendings.lock().await;
        for i in 10..30 {
            timer.delete(pendings[i].0).await.unwrap();
            deleted.push(pendings[i].0);
        }
    }

    // Wait for any remaining items to expire
    tokio::time::sleep(tokio::time::Duration::from_millis(20000)).await;
    let pendings = pendings.lock().await;
    println!("All timers state: {:?}", pendings);
    for pending in pendings.iter() {
        if let Some(_) = deleted.iter().find(|&&x| x == pending.0) {
            assert!(!pending.2, "Deleted timer is fired");
        } else {
            assert!(pending.2, "Not all active timers are fired");
        }
    }

    Ok(())
}
