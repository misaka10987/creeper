use std::{ops::Deref, sync::Arc};

use tokio::sync::{Semaphore, SemaphorePermit};

#[derive(Clone)]
pub struct Throttle<T> {
    inner: T,
    semaphore: Arc<Semaphore>,
}

impl<T> Throttle<T> {
    pub fn new(inner: T, parallel: usize) -> Self {
        Self {
            inner,
            semaphore: Arc::new(Semaphore::new(parallel)),
        }
    }

    pub async fn get(&self) -> Throttled<'_, T> {
        let permit = self.semaphore.acquire().await.expect("semaphore closed");

        Throttled {
            inner: &self.inner,
            permit,
        }
    }

    pub fn unwrap(&self) -> &T {
        &self.inner
    }

    pub fn share<U>(&self, new: U) -> Throttle<U> {
        Throttle {
            inner: new,
            semaphore: self.semaphore.clone(),
        }
    }

    pub fn derive<U>(&self, f: impl FnOnce(&T) -> U) -> Throttle<U> {
        Throttle {
            inner: f(&self.inner),
            semaphore: self.semaphore.clone(),
        }
    }

    pub fn try_derive<U, E>(&self, f: impl FnOnce(&T) -> Result<U, E>) -> Result<Throttle<U>, E> {
        Ok(Throttle {
            inner: f(&self.inner)?,
            semaphore: self.semaphore.clone(),
        })
    }
}

pub struct Throttled<'a, T> {
    inner: &'a T,

    #[allow(unused)]
    permit: SemaphorePermit<'a>,
}

impl<'a, T> Deref for Throttled<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<'a, T> Throttled<'a, T> {
    pub fn as_inner(&self) -> &T {
        self.inner
    }

    pub fn forget(self) -> &'a T {
        self.inner
    }
}
