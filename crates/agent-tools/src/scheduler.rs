use crate::types::{ExecutionDomain, ExecutionPriority, ToolError};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

type BoxedTaskFuture = Pin<Box<dyn Future<Output = Result<Vec<u8>, ToolError>> + Send>>;

#[derive(Debug, Clone)]
pub struct ScheduledTaskMetadata {
    pub queue_wait: Duration,
}

struct PendingTask {
    sequence: u64,
    enqueued_at: Instant,
    priority: ExecutionPriority,
    domain: ExecutionDomain,
    task: BoxedTaskFuture,
    reply: oneshot::Sender<Result<(Vec<u8>, ScheduledTaskMetadata), ToolError>>,
}

impl PartialEq for PendingTask {
    fn eq(&self, other: &Self) -> bool {
        self.sequence == other.sequence
    }
}

impl Eq for PendingTask {}

impl PartialOrd for PendingTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PendingTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

enum SchedulerMessage {
    Enqueue(PendingTask),
    Completed(ExecutionDomain),
}

struct SchedulerConfig {
    total_limit: usize,
    inference_limit: usize,
    tools_limit: usize,
    memory_limit: usize,
    system_limit: usize,
}

impl SchedulerConfig {
    fn load() -> Self {
        Self {
            total_limit: env_limit("APP_AGENT_EXEC_TOTAL_CONCURRENCY", 2),
            inference_limit: env_limit("APP_AGENT_EXEC_INFERENCE_CONCURRENCY", 1),
            tools_limit: env_limit("APP_AGENT_EXEC_TOOLS_CONCURRENCY", 2),
            memory_limit: env_limit("APP_AGENT_EXEC_MEMORY_CONCURRENCY", 1),
            system_limit: env_limit("APP_AGENT_EXEC_SYSTEM_CONCURRENCY", 1),
        }
    }

    fn limit_for(&self, domain: ExecutionDomain) -> usize {
        match domain {
            ExecutionDomain::Inference => self.inference_limit,
            ExecutionDomain::Tools => self.tools_limit,
            ExecutionDomain::Memory => self.memory_limit,
            ExecutionDomain::System => self.system_limit,
        }
    }
}

struct ActiveCounts {
    total: usize,
    inference: usize,
    tools: usize,
    memory: usize,
    system: usize,
}

impl ActiveCounts {
    fn increment(&mut self, domain: ExecutionDomain) {
        self.total += 1;
        match domain {
            ExecutionDomain::Inference => self.inference += 1,
            ExecutionDomain::Tools => self.tools += 1,
            ExecutionDomain::Memory => self.memory += 1,
            ExecutionDomain::System => self.system += 1,
        }
    }

    fn decrement(&mut self, domain: ExecutionDomain) {
        self.total = self.total.saturating_sub(1);
        match domain {
            ExecutionDomain::Inference => self.inference = self.inference.saturating_sub(1),
            ExecutionDomain::Tools => self.tools = self.tools.saturating_sub(1),
            ExecutionDomain::Memory => self.memory = self.memory.saturating_sub(1),
            ExecutionDomain::System => self.system = self.system.saturating_sub(1),
        }
    }

    fn value_for(&self, domain: ExecutionDomain) -> usize {
        match domain {
            ExecutionDomain::Inference => self.inference,
            ExecutionDomain::Tools => self.tools,
            ExecutionDomain::Memory => self.memory,
            ExecutionDomain::System => self.system,
        }
    }
}

struct TaskScheduler {
    tx: mpsc::UnboundedSender<SchedulerMessage>,
    sequence: AtomicU64,
}

impl TaskScheduler {
    fn spawn() -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let thread_tx = tx.clone();
        std::thread::Builder::new()
            .name("mlx-agent-exec-scheduler".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build exec scheduler runtime");
                runtime.block_on(run_dispatcher(rx, thread_tx, SchedulerConfig::load()));
            })
            .expect("failed to spawn exec scheduler thread");
        Arc::new(TaskScheduler {
            tx,
            sequence: AtomicU64::new(1),
        })
    }
}

fn scheduler_handle() -> Arc<TaskScheduler> {
    static INSTANCE: OnceLock<Mutex<Option<Arc<TaskScheduler>>>> = OnceLock::new();
    let slot = INSTANCE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().expect("scheduler mutex poisoned");
    let needs_spawn = guard
        .as_ref()
        .map(|scheduler| scheduler.tx.is_closed())
        .unwrap_or(true);
    if needs_spawn {
        *guard = Some(TaskScheduler::spawn());
    }
    guard
        .as_ref()
        .cloned()
        .expect("scheduler should exist after spawn")
}

pub async fn schedule_exec_task<F>(
    domain: ExecutionDomain,
    priority: ExecutionPriority,
    task: F,
) -> Result<(Vec<u8>, ScheduledTaskMetadata), ToolError>
where
    F: Future<Output = Result<Vec<u8>, ToolError>> + Send + 'static,
{
    let scheduler = scheduler_handle();
    let (reply_tx, reply_rx) = oneshot::channel();
    let pending = PendingTask {
        sequence: scheduler.sequence.fetch_add(1, AtomicOrdering::Relaxed),
        enqueued_at: Instant::now(),
        priority,
        domain,
        task: Box::pin(task),
        reply: reply_tx,
    };
    scheduler
        .tx
        .send(SchedulerMessage::Enqueue(pending))
        .map_err(|_| ToolError::ExecutionFailed {
            message: "failed to enqueue exec task".to_string(),
        })?;
    reply_rx.await.map_err(|_| ToolError::ExecutionFailed {
        message: "exec scheduler dropped task".to_string(),
    })?
}

async fn run_dispatcher(
    mut rx: mpsc::UnboundedReceiver<SchedulerMessage>,
    tx: mpsc::UnboundedSender<SchedulerMessage>,
    config: SchedulerConfig,
) {
    let mut queue = BinaryHeap::<PendingTask>::new();
    let mut active = ActiveCounts {
        total: 0,
        inference: 0,
        tools: 0,
        memory: 0,
        system: 0,
    };

    while let Some(message) = rx.recv().await {
        match message {
            SchedulerMessage::Enqueue(task) => queue.push(task),
            SchedulerMessage::Completed(domain) => active.decrement(domain),
        }

        while active.total < config.total_limit {
            let Some(task) = take_next_runnable(&mut queue, &active, &config) else {
                break;
            };
            active.increment(task.domain);
            let completion_tx = tx.clone();
            tokio::spawn(async move {
                let queue_wait = task.enqueued_at.elapsed();
                let result = task
                    .task
                    .await
                    .map(|value| (value, ScheduledTaskMetadata { queue_wait }));
                let _ = task.reply.send(result);
                let _ = completion_tx.send(SchedulerMessage::Completed(task.domain));
            });
        }
    }
}

fn take_next_runnable(
    queue: &mut BinaryHeap<PendingTask>,
    active: &ActiveCounts,
    config: &SchedulerConfig,
) -> Option<PendingTask> {
    let mut held = Vec::new();
    let mut selected = None;
    while let Some(task) = queue.pop() {
        let domain_active = active.value_for(task.domain);
        if domain_active < config.limit_for(task.domain) {
            selected = Some(task);
            break;
        }
        held.push(task);
    }

    for task in held {
        queue.push(task);
    }

    selected
}

fn env_limit(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}
