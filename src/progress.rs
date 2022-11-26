use std::{collections::BTreeMap, future::Future, sync::Arc};

use tokio::{
    runtime::Runtime,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Mutex, RwLock,
    },
};

use crate::task::TaskId;

#[derive(Debug)]
pub struct ProgressManager {
    receiver: Arc<Mutex<Receiver<TaskProgress>>>,
    sender: Sender<TaskProgress>,
    tasks: RwLock<BTreeMap<TaskId, Vec<TaskId>>>,
    progresses: RwLock<BTreeMap<TaskId, Progress>>,
    parent_id: RwLock<BTreeMap<TaskId, TaskId>>,
}

impl ProgressManager {
    pub fn new() -> Self {
        let (sender, receiver) = channel(100);
        let tasks = RwLock::new(BTreeMap::new());
        let progresses = RwLock::new(BTreeMap::new());
        let parent_id = RwLock::new(BTreeMap::new());

        Self {
            receiver: Arc::new(Mutex::new(receiver)),
            sender,
            tasks,
            progresses,
            parent_id,
        }
    }

    pub fn handle<F, Fut>(&self, rt: &Runtime, f: F)
    where
        F: FnOnce(TaskProgress) -> Fut + Send + Clone + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let receiver = self.receiver.clone();
        rt.spawn(async move {
            while let Some(taskprogress) = receiver.lock().await.recv().await {
                (f.clone())(taskprogress).await;
            }
        });
    }

    pub async fn drop_task(&self, task_id: TaskId) {
        let mut tasks = self.tasks.write().await;
        let mut progresses = self.progresses.write().await;
        let mut parent_id = self.parent_id.write().await;

        if let Some((task, subtasks)) = tasks.remove_entry(&task_id) {
            progresses.remove(&task);

            subtasks.iter().for_each(|task| {
                progresses.remove(task);
                parent_id.remove(task);
            })
        }
    }

    pub async fn add_task(&self, task_id: TaskId, progress: Progress) {
        let mut tasks = self.tasks.write().await;
        let mut progresses = self.progresses.write().await;

        if tasks.get(&task_id).is_none() {
            tasks.insert(task_id.clone(), vec![]);
        }
        progresses.insert(task_id, progress);
    }

    pub async fn add_subtask(&self, task_id: &TaskId, subtask_id: TaskId, progress: Progress) {
        let mut tasks = self.tasks.write().await;
        let mut progresses = self.progresses.write().await;
        let mut parent_id = self.parent_id.write().await;

        if let Some(tasks) = tasks.get_mut(task_id) {
            tasks.push(subtask_id.clone());
            progresses.insert(subtask_id.clone(), progress);
            parent_id.insert(subtask_id, task_id.clone());
        }
    }

    pub async fn update_task(&self, task_id: TaskId, progress: Progress) {
        let mut progresses = self.progresses.write().await;

        if let Some(task_progress) = progresses.get_mut(&task_id) {
            *task_progress = progress;
        }
    }

    pub async fn send_task_progress(&self, task_id: TaskId) {
        let tasks = self.tasks.read().await;
        let progresses = self.progresses.read().await;
        let parent_id = self.parent_id.read().await;
        let mut parent_task_id: TaskId = task_id;

        if let Some(id) = parent_id.get(&parent_task_id) {
            parent_task_id = id.clone();
        }

        let mut subtask_progresses: Vec<Progress> = Vec::new();

        if let Some(task_progress) = progresses.get(&parent_task_id) {
            if let Some(subtasks) = tasks.get(&parent_task_id) {
                subtasks.iter().for_each(|subtask_id| {
                    if let Some(subtask) = progresses.get(subtask_id) {
                        subtask_progresses.push(subtask.to_owned());
                    }
                });
            }

            self.sender
                .send(TaskProgress {
                    task_id: parent_task_id,
                    progress: task_progress.to_owned(),
                    subtask_progress: subtask_progresses,
                })
                .await
                .unwrap_or(()); // don't care about the SendError
        }
    }
}

impl Default for ProgressManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressStatus {
    Waiting,
    Running,
    Error,
    Successful,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub task: String,
    pub status: ProgressStatus,
    pub message: String,
    pub max: u64,
    pub current: u64,
}

impl Progress {
    pub fn new_waiting(task: String, message: &str, max: u64, current: u64) -> Self {
        Self {
            message: message.to_owned(),
            max,
            current,
            task,
            status: ProgressStatus::Waiting,
        }
    }
    pub fn new_running(task: String, message: &str, max: u64, current: u64) -> Self {
        Self {
            message: message.to_owned(),
            max,
            current,
            task,
            status: ProgressStatus::Running,
        }
    }

    pub fn set_max(&mut self, max: u64) {
        self.max = max;
    }

    pub fn set_message(&mut self, text: &str) {
        self.message = text.to_owned()
    }

    pub fn set(&mut self, value: u64) {
        if value > self.max {
            self.set_max(value);
        }
        self.current = value;
    }

    pub fn set_status(&mut self, status: ProgressStatus) {
        self.status = status;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskProgress {
    task_id: TaskId,
    progress: Progress,
    subtask_progress: Vec<Progress>,
}

impl TaskProgress {
    pub fn new(task_id: TaskId, progress: Progress, subtask_progress: Vec<Progress>) -> Self {
        Self {
            task_id,
            progress,
            subtask_progress,
        }
    }
}
