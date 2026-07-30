use std::{ops::Deref, sync::Arc};

use reqwest::Client;
use tokio::sync::{Semaphore, SemaphorePermit};

#[derive(Clone)]
pub struct HttpThrottle {
    client: Client,
    semaphore: Arc<Semaphore>,
}

impl HttpThrottle {
    pub fn new(client: Client, parallel: usize) -> Self {
        Self {
            client,
            semaphore: Arc::new(Semaphore::new(parallel)),
        }
    }

    pub async fn req(&self) -> Request<'_> {
        let permit = self.semaphore.acquire().await.expect("semaphore closed");

        Request {
            client: &self.client,
            permit,
        }
    }

    pub fn as_client(&self) -> &Client {
        &self.client
    }
}

pub struct Request<'a> {
    client: &'a Client,

    /// RAII guard for the semaphore permit.
    #[allow(unused)]
    permit: SemaphorePermit<'a>,
}

impl<'a> Deref for Request<'a> {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        self.client
    }
}
