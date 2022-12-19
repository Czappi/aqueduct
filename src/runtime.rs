use std::{future::Future, io, sync::Arc};

use tokio::{
    runtime::{Builder, Runtime},
    task::JoinHandle,
};
use tracing::instrument;

use crate::{
    progress::{Progress, ProgressManager, TaskProgress},
    task::{handle::TaskHandle, Task},
};

#[derive(Debug)]
pub struct Handle {
    pub progress_manager: ProgressManager,
    rt: Runtime,
}

impl Handle {
    pub fn new(progress_manager: ProgressManager, rt: Runtime) -> Self {
        Self {
            progress_manager,
            rt,
        }
    }

    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.rt.spawn(future)
    }

    pub fn spawn_blocking<F, R>(&self, func: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.rt.spawn_blocking(func)
    }
}

#[derive(Debug, Clone)]
pub struct Aqueduct {
    pub handle: Arc<Handle>,
}

impl Aqueduct {
    pub fn new_multi_threaded() -> io::Result<Self> {
        let rt = Builder::new_multi_thread().enable_all().build()?;

        Ok(Self {
            handle: Arc::new(Handle {
                progress_manager: ProgressManager::new(),
                rt,
            }),
        })
    }

    pub fn new_single_threaded() -> io::Result<Self> {
        let rt = Builder::new_current_thread().enable_all().build()?;

        Ok(Self {
            handle: Arc::new(Handle {
                progress_manager: ProgressManager::new(),
                rt,
            }),
        })
    }

    pub fn new(runtime: Runtime) -> Self {
        Self {
            handle: Arc::new(Handle {
                progress_manager: ProgressManager::new(),
                rt: runtime,
            }),
        }
    }

    pub fn handle_progress<F, Fut>(&self, f: F)
    where
        F: FnOnce(TaskProgress) -> Fut + Send + Clone + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.handle.progress_manager.handle(&self.handle.rt, f);
    }

    #[instrument(skip(self))]
    pub fn spawn<FunctionOutput, Output, T>(&self, task: T) -> Output
    where
        T: Task<FunctionOutput, Output>,
    {
        let handle = TaskHandle::new(
            Aqueduct {
                handle: self.handle.clone(),
            },
            Progress::new_running(task.name(), "", 0, 0),
            task.id(),
        );
        task.spawn(self.clone(), handle)
    }
}
