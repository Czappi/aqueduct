use std::{fmt::Debug, future::Future};

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
    pub fn run<T: Task>(&mut self, subtask: Subtask<T>) -> T::TaskResult {
        self.progress(|progress| {
            progress.set_status(ProgressStatus::Running);
        });

        subtask.task.spawn(self.runtime.clone(), subtask.handle)
    }

    #[instrument(skip_all, fields(task = ?task))]
    pub fn register<T: Task>(&self, task: T) -> Subtask<T> {
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

        Subtask { task, handle }
    }

    #[instrument(skip_all, fields(task = ?task))]
    pub fn spawn<T: Task>(&mut self, task: T) -> T::TaskResult {
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
pub struct Subtask<T: Task> {
    pub task: T,
    pub handle: TaskHandle,
}

pub trait Task: Debug {
    type Output: Send + 'static;
    type TaskFunction: (FnOnce(TaskHandle) -> Self::Output) + ?Sized + Send + 'static;
    type TaskResult;

    fn spawn(self, rt: Aqueduct, task_handle: TaskHandle) -> Self::TaskResult;

    fn new(name: &str, function: Self::TaskFunction) -> Self;

    fn id(&self) -> TaskId;

    fn name(&self) -> String;
}

pub struct BlockingTask<R, F>
where
    F: FnOnce(TaskHandle) -> R + Send + 'static,
    R: Send + 'static,
{
    id: TaskId,
    name: String,
    pub function: F,
}

impl<R, F> Debug for BlockingTask<R, F>
where
    F: FnOnce(TaskHandle) -> R + Send + 'static,
    R: Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockingTask")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

impl<R, F> Task for BlockingTask<R, F>
where
    F: FnOnce(TaskHandle) -> R + Send + 'static,
    R: Send + 'static,
{
    type Output = R;

    type TaskFunction = F;

    type TaskResult = JoinHandle<Self::Output>;

    #[instrument(skip_all, fields(task = ?self))]
    fn spawn(self, rt: Aqueduct, task_handle: TaskHandle) -> Self::TaskResult {
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

pub struct AsyncTask<F, Fut, R>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    R: Send + 'static,
    Fut: Future<Output = R> + Send + 'static,
{
    id: TaskId,
    name: String,
    pub function: F,
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

impl<F, Fut, R> Task for AsyncTask<F, Fut, R>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    R: Send + 'static,
    Fut: Future<Output = R> + Send + 'static,
{
    type Output = Fut;

    type TaskFunction = F;

    type TaskResult = JoinHandle<R>;

    #[instrument(skip_all, fields(task = ?self))]
    fn spawn(self, rt: Aqueduct, task_handle: TaskHandle) -> Self::TaskResult {
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

// TODO: Implement LocalAsyncTask
struct LocalAsyncTask<F, Fut, R>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    R: Send + 'static,
    Fut: Future<Output = R> + Send + 'static,
{
    id: TaskId,
    name: String,
    pub function: F,
}

impl<F, Fut, R> Debug for LocalAsyncTask<F, Fut, R>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    R: Send + 'static,
    Fut: Future<Output = R> + Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalAsyncTask")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

impl<F, Fut, R> Task for LocalAsyncTask<F, Fut, R>
where
    F: FnOnce(TaskHandle) -> Fut + Send + 'static,
    R: Send + 'static,
    Fut: Future<Output = R> + Send + 'static,
{
    type Output = Fut;

    type TaskFunction = F;

    type TaskResult = Self::Output;

    #[instrument(skip_all, fields(task = ?self))]
    fn spawn(self, _rt: Aqueduct, task_handle: TaskHandle) -> Self::TaskResult {
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

struct LocalSyncTask<F, R>
where
    F: (FnOnce(TaskHandle) -> R) + Send + 'static,
    R: Send + 'static,
{
    id: TaskId,
    name: String,
    pub function: F,
}

impl<F, R> Debug for LocalSyncTask<F, R>
where
    F: (FnOnce(TaskHandle) -> R) + Send + 'static,
    R: Send + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSyncTask")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

impl<F, R> Task for LocalSyncTask<F, R>
where
    F: (FnOnce(TaskHandle) -> R) + Send + 'static,
    R: Send + 'static,
{
    type Output = R;

    type TaskFunction = F;

    type TaskResult = Self::Output;

    #[instrument(skip_all, fields(task = ?self))]
    fn spawn(self, _rt: Aqueduct, task_handle: TaskHandle) -> Self::TaskResult {
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

#[test]
fn task_test() {
    use tracing::info;
    use tracing::metadata::LevelFilter;
    use tracing_subscriber::FmtSubscriber;

    let subscriber = FmtSubscriber::builder()
        .pretty()
        .with_thread_ids(true)
        .with_max_level(LevelFilter::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let aque = Aqueduct::new_multi_threaded().unwrap();
    let d = String::from("asdas");
    let atask = AsyncTask::new("async", move |mut handle| async move {
        info!(d);
        let subtask = handle.register(BlockingTask::new("blocking subtask", |_| {
            info!("blocking subtask");
        }));
        handle.run(subtask);
        handle
            .runtime
            .spawn(BlockingTask::new("blocking subtask2", |_| {
                info!("blocking subtask2");
            }));

        let subtask = handle.register(AsyncTask::new("async subtask", |_| async {
            info!("async subtask");
        }));

        handle.run(subtask);

        //handle.runtime.handle.spawn_blocking(|| info!("hehe"));

        info!("async");
    });

    let btask = BlockingTask::new("blocking", |_| {
        info!("blocking");
    });

    let latask = LocalAsyncTask::new("local_async", |_| async {
        info!("local_async");
    });
    let lstask = LocalSyncTask::new("local_sync", |_| {
        info!("local_sync");
    });

    aque.spawn(atask);
    aque.spawn(btask);
    aque.spawn(lstask);
    let latask_future = aque.spawn(latask);

    aque.handle.spawn(async {
        latask_future.await;
    });
}
