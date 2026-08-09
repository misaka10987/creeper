use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, Mutex, RwLock},
};

use tokio::sync::mpsc;

type Queue = Arc<Mutex<Vec<mpsc::UnboundedSender<Option<SingleFlightGuard>>>>>;

pub struct SingleFlightInner {
    map: RwLock<HashMap<String, Queue>>,
}

#[derive(Clone)]
pub struct SingleFlight(Arc<SingleFlightInner>);

impl Deref for SingleFlight {
    type Target = SingleFlightInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl SingleFlight {
    pub fn new() -> Self {
        let inner = SingleFlightInner {
            map: RwLock::new(HashMap::new()),
        };

        Self(Arc::new(inner))
    }

    pub fn queue(&self, key: String) -> SingleFlightQueue {
        let (send, recv) = mpsc::unbounded_channel();

        if let Some(queue) = self.map.read().unwrap().get(&key) {
            let mut queue = queue.lock().unwrap();

            queue.push(send.clone());

            return SingleFlightQueue::new(recv);
        }

        let mut write = self.map.write().unwrap();

        let entry = write.entry(key.clone()).or_default();

        let mut queue = entry.lock().unwrap();

        queue.push(send.clone());

        // since we are holding the mutex lock,
        // no other thread can push to the queue,
        // so we can always assume the value is the one we just pushed
        if queue.len() == 1 {
            send.send(Some(SingleFlightGuard {
                key,
                queue: entry.clone(),
                target: self.clone(),
            }))
            // since `recv` is always there this should never fail
            .unwrap();
        }

        SingleFlightQueue::new(recv)
    }
}

#[derive(Clone)]
pub struct SingleFlightGuard {
    key: String,
    target: SingleFlight,
    queue: Queue,
}

impl Deref for SingleFlightGuard {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.key
    }
}

impl Drop for SingleFlightGuard {
    fn drop(&mut self) {
        let mut queue = self.queue.lock().unwrap();

        if queue.is_empty() {
            self.target.map.write().unwrap().remove(&self.key);
            return;
        }

        let first = queue.remove(0);

        let _ = first.send(Some(self.clone()));

        for i in queue.iter() {
            let _ = i.send(None);
        }
    }
}

pub struct SingleFlightQueue {
    recv: mpsc::UnboundedReceiver<Option<SingleFlightGuard>>,
}

impl SingleFlightQueue {
    pub const fn new(recv: mpsc::UnboundedReceiver<Option<SingleFlightGuard>>) -> Self {
        Self { recv }
    }

    pub async fn advance(&mut self) -> Option<SingleFlightGuard> {
        self.recv.recv().await.unwrap()
    }
}
