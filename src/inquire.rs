use tokio::{
    sync::{Mutex, MutexGuard},
    task::{JoinError, spawn_blocking},
};
use tracing::Metadata;
use tracing_subscriber::{
    filter::{FilterFn, filter_fn},
    reload::Handle,
};

use crate::Creeper;

struct InquireManagerInner {
    start_hooks: Vec<Box<dyn FnMut() + Send>>,
    end_hooks: Vec<Box<dyn FnMut() + Send>>,
}

pub struct InquireManager {
    inner: Mutex<InquireManagerInner>,
}

impl InquireManager {
    pub fn new() -> Self {
        let inner = InquireManagerInner {
            start_hooks: vec![],
            end_hooks: vec![],
        };

        Self {
            inner: Mutex::new(inner),
        }
    }
}

impl Creeper {
    pub async fn before_inquire(&self, hook: impl FnMut() + Send + 'static) {
        self.inquire
            .inner
            .lock()
            .await
            .start_hooks
            .push(Box::new(hook));
    }

    pub fn blocking_before_inquire(&self, hook: impl FnMut() + Send + 'static) {
        self.inquire
            .inner
            .blocking_lock()
            .start_hooks
            .push(Box::new(hook));
    }

    pub async fn after_inquire(&self, hook: impl FnMut() + Send + 'static) {
        self.inquire
            .inner
            .lock()
            .await
            .end_hooks
            .push(Box::new(hook));
    }

    pub fn blocking_after_inquire(&self, hook: impl FnMut() + Send + 'static) {
        self.inquire
            .inner
            .blocking_lock()
            .end_hooks
            .push(Box::new(hook));
    }

    pub async fn inquire(&self) -> InquireGuard<'_> {
        let inner = self.inquire.inner.lock().await;

        self.stdio.disable_by_inquire();

        InquireGuard {
            lib: self.clone(),
            inner,
        }
    }

    pub async fn inquire_filter<S: 'static>(&self, handle: Handle<Filter, S>) {
        let mut inner = self.inquire.inner.lock().await;

        let h = handle.clone();
        inner.start_hooks.push(Box::new(move || {
            let _ = h.reload(make_filter(|_| false));
        }));

        let h = handle.clone();
        inner.end_hooks.push(Box::new(move || {
            let _ = h.reload(make_filter(|_| true));
        }));
    }

    pub fn blocking_inquire_filter<S: 'static>(&self, handle: Handle<Filter, S>) {
        let mut inner = self.inquire.inner.blocking_lock();

        let h = handle.clone();
        inner.start_hooks.push(Box::new(move || {
            let _ = h.reload(make_filter(|_| false));
        }));

        let h = handle.clone();
        inner.end_hooks.push(Box::new(move || {
            let _ = h.reload(make_filter(|_| true));
        }));
    }
}

#[must_use = "The RAII guard must be held during inquire operation."]
pub struct InquireGuard<'a> {
    lib: Creeper,
    inner: MutexGuard<'a, InquireManagerInner>,
}

impl<'a> Drop for InquireGuard<'a> {
    fn drop(&mut self) {
        for hook in self.inner.end_hooks.iter_mut() {
            hook();
        }

        self.lib.stdio.reenable_by_inquire();
    }
}

impl<'a> InquireGuard<'a> {
    pub async fn run<T>(self, f: impl FnOnce() -> T + Send + 'static) -> Result<T, JoinError>
    where
        T: Send + 'static,
    {
        spawn_blocking(f).await
    }
}

pub type Filter = FilterFn<fn(&Metadata<'_>) -> bool>;

pub fn make_filter(f: fn(&Metadata<'_>) -> bool) -> Filter {
    filter_fn(f)
}
