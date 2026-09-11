use std::{ops::Deref, sync::atomic::AtomicU64};

use dashmap::DashMap;
use tokio::sync::watch;

struct SingleFlightLock {
    curr: AtomicU64,
    total: AtomicU64,
    send: watch::Sender<u64>,
}

impl SingleFlightLock {
    pub fn next(&self) {
        let prev = self.curr.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        self.send.send_modify(|x| {
            if *x != prev {
                panic!("single-flight internal inconsistency: current number is {} but previously sent number is {}", prev + 1, *x);
            }

            *x = prev + 1
        });
    }

    pub fn new() -> Self {
        let (send, _) = watch::channel(0);

        Self {
            curr: AtomicU64::new(0),
            total: AtomicU64::new(0),
            send,
        }
    }

    pub fn queue(&self) -> (u64, watch::Receiver<u64>) {
        let prev = self.total.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        (prev + 1, self.send.subscribe())
    }

    pub fn is_done(&self) -> bool {
        self.curr.load(std::sync::atomic::Ordering::SeqCst)
            == self.total.load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub struct SingleFlight {
    map: DashMap<String, SingleFlightLock>,
}

impl SingleFlight {
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    pub fn queue(&self, key: String) -> SingleFlightQueue<'_> {
        let entry = self
            .map
            .entry(key)
            .or_insert_with(SingleFlightLock::new)
            .downgrade();

        let (ticket, recv) = entry.value().queue();

        SingleFlightQueue {
            key: entry.key().clone(),
            ticket,
            recv,
            target: self,
        }
    }
}

pub struct SingleFlightQueue<'a> {
    key: String,
    ticket: u64,
    recv: watch::Receiver<u64>,
    target: &'a SingleFlight,
}

impl<'a> SingleFlightQueue<'a> {
    fn check(&mut self) -> Option<SingleFlightGuard<'a>> {
        let curr = *self.recv.borrow_and_update();

        if curr > self.ticket {
            panic!(
                "single-flight missed queue position {} and advanced to {curr}",
                self.ticket
            )
        }

        if curr == self.ticket {
            return Some(SingleFlightGuard {
                key: self.key.clone(),
                target: self.target,
            });
        }

        None
    }

    /// # Panics
    /// Panic if a guard has already been returned in a previous call,
    /// since no possible future call will return a guard and this method may deadlock.
    pub async fn advance(&mut self) -> Option<SingleFlightGuard<'a>> {
        if let Some(guard) = self.check() {
            return Some(guard);
        }

        self.recv.changed().await.unwrap();

        self.check()
    }
}

pub struct SingleFlightGuard<'a> {
    key: String,
    target: &'a SingleFlight,
}

impl<'a> Deref for SingleFlightGuard<'a> {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.key
    }
}

impl<'a> Drop for SingleFlightGuard<'a> {
    fn drop(&mut self) {
        if self
            .target
            .map
            .remove_if(&self.key, |_k, v| v.is_done())
            .is_some()
        {
            return;
        }

        self.target.map.get(&self.key).unwrap().next();
    }
}
