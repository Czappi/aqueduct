use std::future::Future;

use once_cell::sync::Lazy;
use tokio::{
    runtime::{Builder, Runtime},
    task::JoinHandle,
};

pub static AQUEDUCT_RUNTIME: Lazy<Aqueduct> = Lazy::new(Aqueduct::new);
pub struct Aqueduct {
    pub runtime: Runtime,
}

impl Aqueduct {
    fn new_multi_threaded() -> Self {
        let runtime = Builder::new_multi_thread().enable_all().build().unwrap();
        Self { runtime }
    }

    fn new_single_threaded() -> Self {
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();

        Self { runtime }
    }

    pub fn new() -> Self {
        Aqueduct::new_multi_threaded()
    }

    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }

    pub fn spawn_blocking<F, R>(&self, func: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.runtime.spawn_blocking(func)
    }
}

impl Default for Aqueduct {
    fn default() -> Self {
        Self::new()
    }
}
