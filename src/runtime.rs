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
    #[cfg(feature = "multi-threaded")]
    fn new() -> Self {
        let runtime = Builder::new_multi_thread().enable_all().build().unwrap();
        Self { runtime }
    }

    #[cfg(feature = "single-threaded")]
    fn new() -> Self {
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();

        Self { runtime }
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

    #[cfg(feature = "single-threaded")]
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    #[cfg(feature = "multi-threaded")]
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        let _guard = self.runtime.enter();
        tokio::task::block_in_place(move || self.runtime.block_on(future))
    }
}

impl Default for Aqueduct {
    fn default() -> Self {
        Self::new()
    }
}
