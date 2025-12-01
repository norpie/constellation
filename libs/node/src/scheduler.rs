//! Task scheduling for constellation-node
//!
//! Provides delayed, scheduled, and interval-based task execution with the same
//! extractor pattern used by handlers.

use crate::config::Config;
use crate::node::Data;
use chrono::{DateTime, Utc};
use rand::Rng;
use std::any::{Any, TypeId};
use std::collections::{BinaryHeap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch, RwLock};
use uuid::Uuid;

// ============================================================================
// Public Types
// ============================================================================

/// Defines when and how often a task should run
#[derive(Debug, Clone)]
pub enum Schedule {
    /// Run once after the specified delay
    After(Duration),

    /// Run once at the specified time
    At(DateTime<Utc>),

    /// Run repeatedly at fixed intervals
    Interval {
        period: Duration,
        /// Optional delay before first execution (defaults to running immediately)
        initial_delay: Option<Duration>,
    },

    /// Run repeatedly with randomized intervals (useful for Raft election timeout)
    ///
    /// Each execution is scheduled after a random duration between `min` and `max`.
    RandomInterval {
        min: Duration,
        max: Duration,
    },

    /// Run according to a cron expression (not yet implemented)
    Cron(String),
}

impl Schedule {
    /// Create an interval schedule with no initial delay
    pub fn every(period: Duration) -> Self {
        Schedule::Interval {
            period,
            initial_delay: None,
        }
    }

    /// Create an interval schedule with initial delay
    pub fn every_with_delay(period: Duration, initial_delay: Duration) -> Self {
        Schedule::Interval {
            period,
            initial_delay: Some(initial_delay),
        }
    }

    /// Create a randomized interval schedule (useful for Raft election timeout)
    ///
    /// Each execution is scheduled after a random duration between `min` and `max`.
    pub fn random_interval(min: Duration, max: Duration) -> Self {
        Schedule::RandomInterval { min, max }
    }

    /// Create a one-shot schedule to run after a delay
    pub fn after(delay: Duration) -> Self {
        Schedule::After(delay)
    }

    /// Create a one-shot schedule to run at a specific time
    pub fn at(datetime: DateTime<Utc>) -> Self {
        Schedule::At(datetime)
    }
}

/// Policy for handling task overlap when previous execution is still running
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverlapPolicy {
    /// Skip this execution if previous is still running (default)
    #[default]
    Skip,

    /// Allow concurrent executions
    Allow,
}

/// Unique identifier for a scheduled task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(Uuid);

impl TaskId {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Get the underlying UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Handle returned when scheduling a task, used for cancellation
#[derive(Clone)]
pub struct TaskHandle {
    id: TaskId,
    command_tx: mpsc::Sender<SchedulerCommand>,
}

impl std::fmt::Debug for TaskHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskHandle").field("id", &self.id).finish()
    }
}

impl TaskHandle {
    /// Get the task ID
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Cancel the scheduled task and wait for confirmation
    ///
    /// Returns `true` if the task was found and cancelled, `false` otherwise.
    pub async fn cancel(&self) -> bool {
        let (response_tx, response_rx) = oneshot::channel();
        if self
            .command_tx
            .send(SchedulerCommand::Cancel {
                id: self.id,
                response: Some(response_tx),
            })
            .await
            .is_err()
        {
            return false;
        }
        response_rx.await.unwrap_or(false)
    }

    /// Cancel the task without waiting for confirmation (fire-and-forget)
    ///
    /// This is useful when you want to cancel with a timeout:
    /// ```ignore
    /// tokio::select! {
    ///     result = handle.cancel() => { /* got confirmation */ }
    ///     _ = tokio::time::sleep(Duration::from_secs(1)) => {
    ///         handle.kill(); // give up waiting
    ///     }
    /// }
    /// ```
    pub fn kill(&self) {
        let _ = self.command_tx.try_send(SchedulerCommand::Cancel {
            id: self.id,
            response: None,
        });
    }

    /// Reset the task's timer, recomputing the next run time from now
    ///
    /// This is useful for Raft election timeout - when a heartbeat is received,
    /// the election timer should be reset. For `RandomInterval` schedules, this
    /// will pick a new random delay.
    ///
    /// Returns `true` if the task was found and reset, `false` otherwise.
    pub async fn reset(&self) -> bool {
        let (response_tx, response_rx) = oneshot::channel();
        if self
            .command_tx
            .send(SchedulerCommand::Reset {
                id: self.id,
                response: response_tx,
            })
            .await
            .is_err()
        {
            return false;
        }
        response_rx.await.unwrap_or(false)
    }

    /// Reset the task's timer without waiting for confirmation (fire-and-forget)
    ///
    /// This is useful when you need to reset quickly without blocking,
    /// such as in a hot path when receiving heartbeats.
    pub fn reset_now(&self) {
        let (response_tx, _response_rx) = oneshot::channel();
        let _ = self.command_tx.try_send(SchedulerCommand::Reset {
            id: self.id,
            response: response_tx,
        });
    }
}

/// Status of a scheduled task
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is scheduled but has not run yet
    Pending,
    /// Task is currently executing
    Running,
    /// Task has completed (one-shot tasks only)
    Completed,
    /// Task was cancelled
    Cancelled,
}

/// Information about a scheduled task
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub id: TaskId,
    pub name: Option<String>,
    pub schedule: Schedule,
    pub status: TaskStatus,
    pub run_count: u64,
    pub last_run: Option<Instant>,
    pub last_duration: Option<Duration>,
    pub next_run: Option<Instant>,
    pub currently_running: bool,
    pub skipped_count: u64,
}

/// Context passed to scheduled tasks, providing dependency extraction
pub struct TaskContext {
    data: Arc<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    task_id: TaskId,
    shutdown_rx: watch::Receiver<bool>,
}

impl TaskContext {
    /// Extract shared data by type (same pattern as handlers)
    pub fn extract<T: 'static>(&self) -> Option<Data<T>> {
        self.data
            .get(&TypeId::of::<Data<T>>())
            .and_then(|any| any.downcast_ref::<Data<T>>())
            .cloned()
    }

    /// Check if the node is shutting down
    ///
    /// Tasks can check this periodically for cooperative cancellation.
    pub fn is_shutting_down(&self) -> bool {
        *self.shutdown_rx.borrow()
    }

    /// Get the current task's ID
    pub fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Get the Scheduler for scheduling more tasks
    pub fn scheduler(&self) -> Option<Data<Scheduler>> {
        self.extract::<Scheduler>()
    }
}

// ============================================================================
// Scheduler
// ============================================================================

/// Scheduler for running tasks at specified times or intervals
///
/// The Scheduler is automatically registered as `Data<Scheduler>` and can be
/// extracted in handlers and tasks to schedule new work dynamically.
#[derive(Clone)]
pub struct Scheduler {
    command_tx: mpsc::Sender<SchedulerCommand>,
}

impl Scheduler {
    /// Create a new scheduler, returning both the handle and the command receiver
    ///
    /// The buffer_size parameter sets the channel capacity. This can only be set
    /// at creation time - runtime config changes won't affect it.
    pub(crate) fn new(buffer_size: usize) -> (Self, mpsc::Receiver<SchedulerCommand>) {
        let (command_tx, command_rx) = mpsc::channel(buffer_size);
        (Self { command_tx }, command_rx)
    }

    /// Create a Scheduler from an existing command sender
    ///
    /// This is useful when you need a Scheduler handle but only have the sender.
    pub(crate) fn from_sender(command_tx: mpsc::Sender<SchedulerCommand>) -> Self {
        Self { command_tx }
    }

    /// Get the command sender (for creating TaskHandles)
    pub(crate) fn command_tx(&self) -> mpsc::Sender<SchedulerCommand> {
        self.command_tx.clone()
    }

    /// Schedule a task
    pub async fn schedule<F, Fut>(&self, schedule: Schedule, task: F) -> crate::Result<TaskHandle>
    where
        F: Fn(TaskContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.schedule_inner(None, schedule, OverlapPolicy::default(), Arc::new(task))
            .await
    }

    /// Schedule a named task
    pub async fn schedule_named<F, Fut>(
        &self,
        name: impl Into<String>,
        schedule: Schedule,
        task: F,
    ) -> crate::Result<TaskHandle>
    where
        F: Fn(TaskContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.schedule_inner(
            Some(name.into()),
            schedule,
            OverlapPolicy::default(),
            Arc::new(task),
        )
        .await
    }

    /// Schedule a task with custom overlap policy
    pub async fn schedule_with_policy<F, Fut>(
        &self,
        schedule: Schedule,
        policy: OverlapPolicy,
        task: F,
    ) -> crate::Result<TaskHandle>
    where
        F: Fn(TaskContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.schedule_inner(None, schedule, policy, Arc::new(task))
            .await
    }

    async fn schedule_inner(
        &self,
        name: Option<String>,
        schedule: Schedule,
        policy: OverlapPolicy,
        task: Arc<dyn TaskFn>,
    ) -> crate::Result<TaskHandle> {
        // Validate cron is not used
        if matches!(schedule, Schedule::Cron(_)) {
            return Err(crate::Error::CronNotImplemented);
        }

        let id = TaskId::new();
        let (response_tx, response_rx) = oneshot::channel();

        self.command_tx
            .send(SchedulerCommand::Schedule {
                id,
                name,
                schedule,
                policy,
                task,
                response: response_tx,
            })
            .await
            .map_err(|_| crate::Error::Scheduler("Scheduler channel closed".into()))?;

        response_rx
            .await
            .map_err(|_| crate::Error::Scheduler("Failed to receive response".into()))
    }

    /// List all scheduled tasks
    pub async fn list(&self) -> Vec<TaskInfo> {
        let (response_tx, response_rx) = oneshot::channel();
        if self
            .command_tx
            .send(SchedulerCommand::List {
                response: response_tx,
            })
            .await
            .is_err()
        {
            return Vec::new();
        }
        response_rx.await.unwrap_or_default()
    }

    /// Get info about a specific task
    pub async fn get(&self, id: TaskId) -> Option<TaskInfo> {
        let (response_tx, response_rx) = oneshot::channel();
        if self
            .command_tx
            .send(SchedulerCommand::Get {
                id,
                response: response_tx,
            })
            .await
            .is_err()
        {
            return None;
        }
        response_rx.await.ok().flatten()
    }

    /// Get a handle for a task by ID
    ///
    /// This creates a new `TaskHandle` that can be used to cancel or reset the task.
    /// Note: This does not verify the task exists - use `get()` first if you need to check.
    pub fn handle_for(&self, id: TaskId) -> TaskHandle {
        TaskHandle {
            id,
            command_tx: self.command_tx.clone(),
        }
    }

    /// Get a handle for a named task
    ///
    /// Looks up a task by name and returns a handle that can be used to cancel or reset it.
    /// Returns `None` if no task with that name exists.
    ///
    /// # Example
    /// ```ignore
    /// // Schedule a named task
    /// scheduler.schedule_named("election_timeout", schedule, task).await?;
    ///
    /// // Later, get a handle to reset it
    /// if let Some(handle) = scheduler.handle_by_name("election_timeout").await {
    ///     handle.reset_now();
    /// }
    /// ```
    pub async fn handle_by_name(&self, name: &str) -> Option<TaskHandle> {
        let tasks = self.list().await;
        tasks
            .iter()
            .find(|t| t.name.as_deref() == Some(name))
            .map(|t| self.handle_for(t.id))
    }
}

// ============================================================================
// Internal Types
// ============================================================================

/// Internal trait for task functions (object-safe wrapper)
pub(crate) trait TaskFn: Send + Sync {
    fn call(&self, ctx: TaskContext) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

impl<F, Fut> TaskFn for F
where
    F: Fn(TaskContext) -> Fut + Send + Sync,
    Fut: Future<Output = ()> + Send + 'static,
{
    fn call(&self, ctx: TaskContext) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin((self)(ctx))
    }
}

pub(crate) enum SchedulerCommand {
    Schedule {
        id: TaskId,
        name: Option<String>,
        schedule: Schedule,
        policy: OverlapPolicy,
        task: Arc<dyn TaskFn>,
        response: oneshot::Sender<TaskHandle>,
    },
    Cancel {
        id: TaskId,
        response: Option<oneshot::Sender<bool>>,
    },
    Reset {
        id: TaskId,
        response: oneshot::Sender<bool>,
    },
    List {
        response: oneshot::Sender<Vec<TaskInfo>>,
    },
    Get {
        id: TaskId,
        response: oneshot::Sender<Option<TaskInfo>>,
    },
    TaskCompleted {
        id: TaskId,
        duration: Duration,
    },
}

struct TaskState {
    id: TaskId,
    name: Option<String>,
    schedule: Schedule,
    task: Arc<dyn TaskFn>,
    overlap_policy: OverlapPolicy,
    run_count: u64,
    last_run: Option<Instant>,
    last_duration: Option<Duration>,
    next_run: Option<Instant>,
    skipped_count: u64,
    cancelled: bool,
    /// Generation counter - incremented on reset to invalidate old pending entries
    generation: u64,
}

impl TaskState {
    fn to_info(&self, running: bool) -> TaskInfo {
        let status = if self.cancelled {
            TaskStatus::Cancelled
        } else if running {
            TaskStatus::Running
        } else if self.next_run.is_none() && self.run_count > 0 {
            TaskStatus::Completed
        } else {
            TaskStatus::Pending
        };

        TaskInfo {
            id: self.id,
            name: self.name.clone(),
            schedule: self.schedule.clone(),
            status,
            run_count: self.run_count,
            last_run: self.last_run,
            last_duration: self.last_duration,
            next_run: self.next_run,
            currently_running: running,
            skipped_count: self.skipped_count,
        }
    }
}

#[derive(PartialEq, Eq)]
struct PendingTask {
    id: TaskId,
    next_run: Instant,
    /// Generation at the time this entry was created - used to invalidate stale entries on reset
    generation: u64,
}

impl Ord for PendingTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reversed for min-heap (earliest first)
        other.next_run.cmp(&self.next_run)
    }
}

impl PartialOrd for PendingTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ============================================================================
// Scheduler Loop
// ============================================================================

/// Run the scheduler loop (spawned as a background task in Node::start())
pub(crate) async fn run_scheduler_loop(
    mut command_rx: mpsc::Receiver<SchedulerCommand>,
    data: Arc<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    mut shutdown_rx: watch::Receiver<bool>,
    command_tx: mpsc::Sender<SchedulerCommand>,
) {
    let mut tasks: HashMap<TaskId, TaskState> = HashMap::new();
    let mut pending: BinaryHeap<PendingTask> = BinaryHeap::new();
    let mut running: HashMap<TaskId, usize> = HashMap::new();

    loop {
        // Calculate time until next task
        // Read idle_sleep_secs from config (if no tasks pending)
        let default_sleep = data
            .get(&TypeId::of::<Data<RwLock<Config>>>())
            .and_then(|any| any.downcast_ref::<Data<RwLock<Config>>>())
            .map(|cfg| {
                // Use try_read to avoid blocking - fall back to default if locked
                cfg.try_read()
                    .map(|c| Duration::from_secs(c.scheduler.idle_sleep_secs))
                    .unwrap_or(Duration::from_secs(3600))
            })
            .unwrap_or(Duration::from_secs(3600));

        let sleep_duration = pending
            .peek()
            .map(|p| p.next_run.saturating_duration_since(Instant::now()))
            .unwrap_or(default_sleep);

        tokio::select! {
            biased;

            // Shutdown signal - highest priority
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }

            // Command from Scheduler API
            Some(cmd) = command_rx.recv() => {
                handle_command(
                    cmd,
                    &mut tasks,
                    &mut pending,
                    &mut running,
                    &command_tx,
                );
            }

            // Timer fired - check for tasks to run
            _ = tokio::time::sleep(sleep_duration) => {
                run_due_tasks(
                    &mut tasks,
                    &mut pending,
                    &mut running,
                    &data,
                    &shutdown_rx,
                    &command_tx,
                );
            }
        }
    }
}

fn handle_command(
    cmd: SchedulerCommand,
    tasks: &mut HashMap<TaskId, TaskState>,
    pending: &mut BinaryHeap<PendingTask>,
    running: &mut HashMap<TaskId, usize>,
    command_tx: &mpsc::Sender<SchedulerCommand>,
) {
    match cmd {
        SchedulerCommand::Schedule {
            id,
            name,
            schedule,
            policy,
            task,
            response,
        } => {
            let next_run = compute_next_run(&schedule, None);

            let task_state = TaskState {
                id,
                name,
                schedule,
                task,
                overlap_policy: policy,
                run_count: 0,
                last_run: None,
                last_duration: None,
                next_run,
                skipped_count: 0,
                cancelled: false,
                generation: 0,
            };

            if let Some(next) = next_run {
                pending.push(PendingTask { id, next_run: next, generation: 0 });
            }

            tasks.insert(id, task_state);

            let handle = TaskHandle {
                id,
                command_tx: command_tx.clone(),
            };
            let _ = response.send(handle);
        }

        SchedulerCommand::Cancel { id, response } => {
            let found = if let Some(task) = tasks.get_mut(&id) {
                task.cancelled = true;
                task.next_run = None;
                true
            } else {
                false
            };
            if let Some(response) = response {
                let _ = response.send(found);
            }
        }

        SchedulerCommand::Reset { id, response } => {
            let found = if let Some(task) = tasks.get_mut(&id) {
                if !task.cancelled {
                    // Increment generation to invalidate any existing pending entries
                    task.generation += 1;
                    // Recompute next run from now (for RandomInterval this picks a new random delay)
                    let next = compute_next_run(&task.schedule, None);
                    task.next_run = next;
                    if let Some(next_run) = next {
                        pending.push(PendingTask { id, next_run, generation: task.generation });
                    }
                    true
                } else {
                    false // Can't reset a cancelled task
                }
            } else {
                false
            };
            let _ = response.send(found);
        }

        SchedulerCommand::List { response } => {
            let infos: Vec<TaskInfo> = tasks
                .values()
                .map(|t| t.to_info(running.contains_key(&t.id)))
                .collect();
            let _ = response.send(infos);
        }

        SchedulerCommand::Get { id, response } => {
            let info = tasks.get(&id).map(|t| t.to_info(running.contains_key(&t.id)));
            let _ = response.send(info);
        }

        SchedulerCommand::TaskCompleted { id, duration } => {
            // Decrement running count
            if let Some(count) = running.get_mut(&id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    running.remove(&id);
                }
            }

            if let Some(task) = tasks.get_mut(&id) {
                task.run_count += 1;
                task.last_run = Some(Instant::now());
                task.last_duration = Some(duration);

                // For non-interval tasks (one-shot), compute and schedule next run
                // Interval/RandomInterval tasks are scheduled immediately when spawned in run_due_tasks
                if !task.cancelled {
                    if !matches!(task.schedule, Schedule::Interval { .. } | Schedule::RandomInterval { .. }) {
                        task.next_run = compute_next_run(&task.schedule, task.last_run);
                        if let Some(next) = task.next_run {
                            pending.push(PendingTask { id, next_run: next, generation: task.generation });
                        }
                    }
                }
            }
        }
    }
}

fn run_due_tasks(
    tasks: &mut HashMap<TaskId, TaskState>,
    pending: &mut BinaryHeap<PendingTask>,
    running: &mut HashMap<TaskId, usize>,
    data: &Arc<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
    shutdown_rx: &watch::Receiver<bool>,
    command_tx: &mpsc::Sender<SchedulerCommand>,
) {
    let now = Instant::now();

    while let Some(next) = pending.peek() {
        if next.next_run > now {
            break;
        }

        let pending_task = pending.pop().unwrap();
        let id = pending_task.id;

        let Some(task_state) = tasks.get_mut(&id) else {
            continue;
        };

        if task_state.cancelled {
            continue;
        }

        // Check if this pending entry is stale (from before a reset)
        if pending_task.generation != task_state.generation {
            continue;
        }

        // Check overlap policy
        let generation = task_state.generation;
        if running.contains_key(&id) {
            match task_state.overlap_policy {
                OverlapPolicy::Skip => {
                    task_state.skipped_count += 1;
                    // Re-schedule for next interval
                    if let Some(next) = compute_next_run(&task_state.schedule, Some(now)) {
                        task_state.next_run = Some(next);
                        pending.push(PendingTask { id, next_run: next, generation });
                    }
                    continue;
                }
                OverlapPolicy::Allow => {
                    // Continue to spawn another instance
                }
            }
        }

        // Increment running count
        *running.entry(id).or_insert(0) += 1;

        // For interval/random interval tasks, immediately schedule next run so we can have concurrent
        // executions (for Allow policy) or detect overlap (for Skip policy)
        match &task_state.schedule {
            Schedule::Interval { period, .. } => {
                let next = now + *period;
                task_state.next_run = Some(next);
                pending.push(PendingTask { id, next_run: next, generation });
            }
            Schedule::RandomInterval { min, max } => {
                let delay = random_duration(*min, *max);
                let next = now + delay;
                task_state.next_run = Some(next);
                pending.push(PendingTask { id, next_run: next, generation });
            }
            _ => {}
        }

        let ctx = TaskContext {
            data: Arc::clone(data),
            task_id: id,
            shutdown_rx: shutdown_rx.clone(),
        };

        let task_fn = Arc::clone(&task_state.task);
        let command_tx = command_tx.clone();

        tokio::spawn(async move {
            let start = Instant::now();
            task_fn.call(ctx).await;
            let duration = start.elapsed();

            let _ = command_tx
                .send(SchedulerCommand::TaskCompleted { id, duration })
                .await;
        });
    }
}

fn compute_next_run(schedule: &Schedule, last_run: Option<Instant>) -> Option<Instant> {
    let now = Instant::now();

    match schedule {
        Schedule::After(delay) => {
            if last_run.is_some() {
                None // One-shot, already ran
            } else {
                Some(now + *delay)
            }
        }
        Schedule::At(datetime) => {
            if last_run.is_some() {
                None // One-shot, already ran
            } else {
                let now_utc = Utc::now();
                if *datetime > now_utc {
                    (*datetime - now_utc)
                        .to_std()
                        .ok()
                        .map(|d| now + d)
                } else {
                    Some(now) // Run immediately if time has passed
                }
            }
        }
        Schedule::Interval {
            period,
            initial_delay,
        } => match last_run {
            None => {
                let delay = initial_delay.unwrap_or(Duration::ZERO);
                Some(now + delay)
            }
            Some(last) => Some(last + *period),
        },
        Schedule::RandomInterval { min, max } => {
            // Always compute from now with a random delay between min and max
            let delay = random_duration(*min, *max);
            Some(now + delay)
        }
        Schedule::Cron(_) => None, // Not implemented
    }
}

/// Generate a random duration between min and max (inclusive)
fn random_duration(min: Duration, max: Duration) -> Duration {
    let mut rng = rand::rng();
    let min_nanos = min.as_nanos() as u64;
    let max_nanos = max.as_nanos() as u64;
    let random_nanos = rng.random_range(min_nanos..=max_nanos);
    Duration::from_nanos(random_nanos)
}

// ============================================================================
// Builder Task Configuration (for NodeBuilder)
// ============================================================================

/// Configuration for a task registered via NodeBuilder
pub(crate) struct ScheduledTaskConfig {
    pub(crate) name: Option<String>,
    pub(crate) schedule: Schedule,
    pub(crate) policy: OverlapPolicy,
    pub(crate) task: Arc<dyn TaskFn>,
}
