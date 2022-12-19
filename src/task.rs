use std::{fmt::Debug, future::Future, marker::PhantomData, pin::Pin};

use lazy_id::Id;
use tokio::task::JoinHandle;
use tracing::instrument;

use crate::{
    progress::{Progress, ProgressStatus},
    runtime::Aqueduct,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(u64);

impl TaskId {
    pub fn new() -> Self {
        TaskId(Id::new().get())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct TaskHandle {
    pub runtime: Aqueduct,
    pub progress: Progress,
    pub task_id: TaskId,
    send_progress: bool,
}

impl TaskHandle {
    pub fn new(runtime: Aqueduct, progress: Progress, task_id: TaskId) -> Self {
        Self {
            runtime,
            progress,
            task_id,
            send_progress: true,
        }
    }

    pub fn new_silent(runtime: Aqueduct, progress: Progress, task_id: TaskId) -> Self {
        Self {
            runtime,
            progress,
            task_id,
            send_progress: false,
        }
    }

    fn subtask_handle(&self, progress: Progress, task_id: TaskId) -> Self {
        Self {
            runtime: self.runtime.clone(),
            progress,
            task_id,
            send_progress: self.send_progress,
        }
    }

    #[instrument(skip_all, fields(subtask = ?subtask.task))]
    pub fn run<FunctionOutput, Output, T: Task<FunctionOutput, Output>>(
        &mut self,
        subtask: Subtask<FunctionOutput, Output, T>,
    ) -> Output {
        self.progress(|progress| {
            progress.set_status(ProgressStatus::Running);
        });

        subtask.task.spawn(self.runtime.clone(), subtask.handle)
    }

    #[instrument(skip_all, fields(task = ?task))]
    pub fn register<FunctionOutput, Output, T: Task<FunctionOutput, Output>>(
        &self,
        task: T,
    ) -> Subtask<FunctionOutput, Output, T> {
        let handle = self.subtask_handle(Progress::new_waiting(task.name(), "", 0, 0), task.id());

        let progress = handle.progress.clone();
        let rt = self.runtime.clone();
        let subtask_id = task.id();
        let task_id = self.task_id.clone();

        if self.send_progress {
            self.runtime.handle.spawn(async move {
                rt.handle
                    .progress_manager
                    .add_subtask(&task_id, subtask_id, progress)
                    .await;
                rt.handle.progress_manager.send_task_progress(task_id).await;
            });
        }

        Subtask {
            task,
            handle,
            _output: PhantomData,
            _function_output: PhantomData,
        }
    }

    #[instrument(skip_all, fields(task = ?task))]
    pub fn spawn<FunctionOutput, Output, T: Task<FunctionOutput, Output>>(
        &mut self,
        task: T,
    ) -> Output {
        let subtask = self.register(task);
        self.run(subtask)
    }

    #[instrument(skip_all, fields(previous_progress = ?self.progress))]
    pub fn progress<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Progress),
    {
        let mut progress = self.progress.clone();

        (f)(&mut progress);

        self.progress = progress.clone();

        let rt = self.runtime.clone();
        let task_id = self.task_id.clone();

        if self.send_progress {
            self.runtime.handle.spawn(async move {
                rt.handle
                    .progress_manager
                    .update_task(task_id.clone(), progress)
                    .await;
                rt.handle.progress_manager.send_task_progress(task_id).await;
            });
        }
    }
}

#[derive(Debug, Clone)]
pub struct Subtask<FunctionOutput, Output, T: Task<FunctionOutput, Output>> {
    pub task: T,
    pub handle: TaskHandle,
    _output: PhantomData<Output>,
    _function_output: PhantomData<FunctionOutput>,
}

pub trait Task<T, R>: Debug {
    type TaskFunction: FnOnce(TaskHandle) -> Self::TaskOutput + Send + 'static;
    type TaskOutput: Send + 'static;

    fn spawn(self, rt: Aqueduct, task_handle: TaskHandle) -> R;

    fn new(name: &str, function: Self::TaskFunction) -> Self;

    fn id(&self) -> TaskId;

    fn name(&self) -> String;
}

pub trait LocalTask<T>: Task<T, T> {}

pub trait TokioTask<T>: Task<T, JoinHandle<T>> {}

pub struct BlockingTask<F, T>
where
    F: FnOnce(TaskHandle) -> T + Send + 'static,
    T: Send + 'static,
{
    id: TaskId,
    name: String,
    pub function: F,
}

impl<F, T> Task<T, JoinHandle<T>> for BlockingTask<F, T>
where
    F: FnOnce(TaskHandle) -> T + Send + 'static,
    T: Send + 'static,
{
    type TaskFunction = F;
    type TaskOutput = T;

    #[instrument(skip_all, fields(task = ?self))]
    fn spawn(self, rt: Aqueduct, task_handle: TaskHandle) -> JoinHandle<T> {
        rt.handle
            .spawn_blocking(move || (self.function)(task_handle))
    }

    #[instrument(skip(function))]
    fn new(name: &str, function: Self::TaskFunction) -> Self {
        BlockingTask {
            id: TaskId::new(),
            name: String::from(name),
            function,
        }
    }

    fn id(&self) -> TaskId {
        self.id.to_owned()
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

impl<F, T> TokioTask<T> for BlockingTask<F, T>
where
    F: FnOnce(TaskHandle) -> T + Send + 'static,
    T: Send + 'static,
{
}

impl<F, T> Debug for BlockingTask<F, T>
where
    F: FnOnce(TaskHandle) -> T + Send + 'static,
    T: Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingTask")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

pub struct AsyncTask<F, Fut, T>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    T: Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    id: TaskId,
    name: String,
    pub function: F,
}

impl<F, Fut, T> Task<T, JoinHandle<T>> for AsyncTask<F, Fut, T>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    T: Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    type TaskFunction = F;
    type TaskOutput = Fut;

    #[instrument(skip_all, fields(task = ?self))]
    fn spawn(self, rt: Aqueduct, task_handle: TaskHandle) -> JoinHandle<T> {
        rt.handle.spawn((self.function)(task_handle))
    }

    #[instrument(skip(function))]
    fn new(name: &str, function: Self::TaskFunction) -> Self {
        Self {
            id: TaskId::new(),
            name: name.to_owned(),
            function,
        }
    }

    fn id(&self) -> TaskId {
        self.id.to_owned()
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

impl<F, Fut, R> TokioTask<R> for AsyncTask<F, Fut, R>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    R: Send + 'static,
    Fut: Future<Output = R> + Send + 'static,
{
}

impl<F, Fut, R> Debug for AsyncTask<F, Fut, R>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    R: Send + 'static,
    Fut: Future<Output = R> + Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncTask")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

// TODO: Implement LocalAsyncTask
struct LocalAsyncTask<F, Fut, T>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    T: Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    id: TaskId,
    name: String,
    pub function: F,
}

impl<F, Fut, T> LocalTask<BoxFuture<T>> for LocalAsyncTask<F, Fut, T>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    T: Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
}

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T>>>;

impl<F, Fut, T> Debug for LocalAsyncTask<F, Fut, T>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    T: Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalAsyncTask")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

impl<F, Fut, T> Task<BoxFuture<T>, BoxFuture<T>> for LocalAsyncTask<F, Fut, T>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    T: Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    type TaskFunction = F;
    type TaskOutput = Fut;

    #[instrument(skip_all, fields(task = ?self))]
    fn spawn(self, _rt: Aqueduct, task_handle: TaskHandle) -> BoxFuture<T> {
        Box::pin((self.function)(task_handle))
    }

    #[instrument(skip(function))]
    fn new(name: &str, function: Self::TaskFunction) -> Self {
        Self {
            id: TaskId::new(),
            name: name.to_owned(),
            function,
        }
    }

    fn id(&self) -> TaskId {
        self.id.to_owned()
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

pub struct LocalSyncTask<F, T>
where
    F: (FnOnce(TaskHandle) -> T) + Send + 'static,
    T: Send + 'static,
{
    id: TaskId,
    name: String,
    pub function: F,
}

impl<F, T> LocalTask<T> for LocalSyncTask<F, T>
where
    F: (FnOnce(TaskHandle) -> T) + Send + 'static,
    T: Send + 'static,
{
}

impl<F, T> Debug for LocalSyncTask<F, T>
where
    F: (FnOnce(TaskHandle) -> T) + Send + 'static,
    T: Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSyncTask")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

impl<F, T> Task<T, T> for LocalSyncTask<F, T>
where
    F: (FnOnce(TaskHandle) -> T) + Send + 'static,
    T: Send + 'static,
{
    type TaskFunction = F;
    type TaskOutput = T;

    #[instrument(skip_all, fields(task = ?self))]
    fn spawn(self, _rt: Aqueduct, task_handle: TaskHandle) -> T {
        (self.function)(task_handle)
    }

    #[instrument(skip(function))]
    fn new(name: &str, function: Self::TaskFunction) -> Self {
        Self {
            id: TaskId::new(),
            name: name.to_owned(),
            function,
        }
    }

    fn id(&self) -> TaskId {
        self.id.to_owned()
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

#[cfg(test)]
static AQUE: once_cell::sync::OnceCell<Aqueduct> = once_cell::sync::OnceCell::new();

#[test]
fn task_test() {
    use tracing::info;
    use tracing::metadata::LevelFilter;
    use tracing_subscriber::FmtSubscriber;

    fn test_blocking() -> impl TokioTask<String> {
        BlockingTask::new("test_blocking", |_| {
            info!("test_blocking");
            String::from("test_blocking")
        })
    }

    fn test_async() -> impl TokioTask<String> {
        AsyncTask::new("test_async", |_| async {
            info!("test_async");
            String::from("test_async")
        })
    }

    fn test_local_async() -> impl LocalTask<BoxFuture<String>> {
        LocalAsyncTask::new("test_local_async", |_| async {
            info!("test_local_async");
            String::from("test_local_async")
        })
    }

    fn test_local_sync() -> impl LocalTask<String> {
        LocalSyncTask::new("test_local_sync", |_| {
            info!("test_local_sync");
            String::from("test_local_sync")
        })
    }
    fn test_subtask() -> impl TokioTask<String> {
        AsyncTask::new("test_subtask", |mut handle| async move {
            handle.spawn(LocalSyncTask::new("test_subtask", |_| {
                info!("test_subtask");
                String::from("test_subtask")
            }))
        })
    }

    let subscriber = FmtSubscriber::builder()
        .pretty()
        .with_thread_ids(true)
        .with_max_level(LevelFilter::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let aque_runtime = Aqueduct::new_multi_threaded().unwrap();

    let _blocking = AQUE
        .get_or_init(|| aque_runtime.clone())
        .spawn(test_blocking());
    let _async_task = AQUE
        .get_or_init(|| aque_runtime.clone())
        .spawn(test_async());
    let _local_async = AQUE
        .get_or_init(|| aque_runtime.clone())
        .spawn(test_local_async());
    let _subtask = AQUE
        .get_or_init(|| aque_runtime.clone())
        .spawn(test_subtask());
    let _local_sync = AQUE
        .get_or_init(|| aque_runtime.clone())
        .spawn(test_local_sync());
}
