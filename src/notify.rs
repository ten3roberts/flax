use core::{
    future::Future,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    task::{Poll, Waker},
};
use std::sync::Mutex;

use alloc::{collections::BTreeMap, sync::Arc};

type LockId = usize;

struct Waiter {
    woken: bool,
    waker: Waker,
}

#[derive(Default)]
pub struct AsyncSignal {
    wakers: Mutex<BTreeMap<LockId, Waiter>>,
    ready: AtomicBool,
    next_id: AtomicUsize,
}

#[derive(Clone)]
/// Provides a mechanism for manually notifying and waking tasks.
pub struct Notify {
    signal: Arc<AsyncSignal>,
}

impl Notify {
    /// Creates a new notify
    pub const fn new() -> Self {
        Self {
            signal: Arc::new(AsyncSignal {
                wakers: Mutex::default(),
                ready: AtomicBool::new(false),
                next_id: AtomicUsize::new(0),
            }),
        }
    }

    /// Notifies the first waiting task
    ///
    /// If no task is waiting, the next call to [`Self::notified`] will resolve immediately
    pub fn notify(&self) {
        // Notify the first waiter
        let guard = self.signal.lock().unwrap();
        if let Some((_, waiter)) = guard.wakers.first_key_value() {
            waiter.woken = true;
            waiter.waker.wake_by_ref();
        } else {
            // Store a permit
            self.signal.ready.store(true, Ordering::SeqCst)
        }
    }

    /// Notifies all *currently* pending tasks.
    pub fn notify_all(&self) {
        let guard = self.signal.lock().unwrap();

        // Notify all current wakers.
        for waiter in guard.wakers.values_mut() {
            waiter.woken = true;
            waiter.waker.wake_by_ref();
        }
    }

    /// Wait until notified.
    pub fn notified(&self) -> Notified {
        Notified {
            id: None,
            signal: &self.signal,
        }
    }
}

struct Notified<'a> {
    id: Option<LockId>,
    signal: &'a AsyncSignal,
}

impl<'a> Future for Notified<'a> {
    type Output = ();

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        // Take ready permit if available
        if self
            .signal
            .ready
            .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return Poll::Ready(());
        }

        if let Some(id) = self.id {
            let guard = self.signal.wakers.lock().unwrap();
            let waiter = &mut guard.wakers[&id];
            // Polled again, return ready or replace the waker
            if waiter.woken {
                guard.remove(&id).unwrap();
                return Poll::Ready(());
            } else {
                waiter.waker = cx.waker().clone();
            }

            Poll::Pending
        } else {
            // Insert a waker and get a place in the queue
            let id = self.signal.next_id.fetch_add(1, Ordering::Relaxed);

            self.signal.lock().unwrap().wakers.insert(
                id,
                Waiter {
                    woken: false,
                    waker: cx.waker().clone(),
                },
            );

            Poll::Pending
        }
    }
}

#[cfg(test)]
mod test {
    use core::time::Duration;

    use tokio::time::sleep;

    use super::*;

    #[tokio::test]
    async fn notify() {
        let notify = Notify::new();
        let notify2 = notify.clone();

        tokio::spawn(async move {
            sleep(Duration::from_millis(500));
            notify2.notify();
        });

        notify.notified().await;
    }
}
