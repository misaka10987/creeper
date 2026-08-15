use tokio::{
    sync::MutexGuard,
    task::{JoinError, spawn_blocking},
};
use tracing_subscriber::reload::Handle;

use crate::{
    Creeper,
    inquire::{Filter, Hooks, make_filter},
};

impl Creeper {
    pub async fn before_inquire(&self, hook: impl FnMut() + Send + 'static) {
        self.inquire.hooks.lock().await.start.push(Box::new(hook));
    }

    pub fn blocking_before_inquire(&self, hook: impl FnMut() + Send + 'static) {
        self.inquire
            .hooks
            .blocking_lock()
            .start
            .push(Box::new(hook));
    }

    pub async fn after_inquire(&self, hook: impl FnMut() + Send + 'static) {
        self.inquire.hooks.lock().await.end.push(Box::new(hook));
    }

    pub fn blocking_after_inquire(&self, hook: impl FnMut() + Send + 'static) {
        self.inquire.hooks.blocking_lock().end.push(Box::new(hook));
    }

    pub async fn inquire(&self) -> InquireGuard<'_> {
        self.inquire
            .active
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let hooks = self.inquire.hooks.lock().await;

        InquireGuard {
            lib: self.clone(),
            hooks,
        }
    }

    pub async fn inquire_filter<S: 'static>(&self, handle: Handle<Filter, S>) {
        let mut hooks = self.inquire.hooks.lock().await;

        let h = handle.clone();
        hooks.start.push(Box::new(move || {
            let _ = h.reload(make_filter(|_| false));
        }));

        let h = handle.clone();
        hooks.end.push(Box::new(move || {
            let _ = h.reload(make_filter(|_| true));
        }));
    }

    pub fn blocking_inquire_filter<S: 'static>(&self, handle: Handle<Filter, S>) {
        let mut hooks = self.inquire.hooks.blocking_lock();

        let h = handle.clone();
        hooks.start.push(Box::new(move || {
            let _ = h.reload(make_filter(|_| false));
        }));

        let h = handle.clone();
        hooks.end.push(Box::new(move || {
            let _ = h.reload(make_filter(|_| true));
        }));
    }
}

#[must_use = "The RAII guard must be held during inquire operation."]
pub struct InquireGuard<'a> {
    lib: Creeper,
    hooks: MutexGuard<'a, Hooks>,
}

impl<'a> Drop for InquireGuard<'a> {
    fn drop(&mut self) {
        self.lib
            .inquire
            .active
            .store(false, std::sync::atomic::Ordering::SeqCst);

        for hook in self.hooks.end.iter_mut() {
            hook();
        }
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
