use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use crate::db::Database;
use crate::profile::scope::ProfileLease;

const LEARNER_QUEUE_CAPACITY: usize = 16;
const INSTRUCTOR_QUEUE_CAPACITY: usize = 8;
const BACKGROUND_QUEUE_CAPACITY: usize = 8;
const FAIR_SCHEDULE: [DatabaseWorkload; 13] = [
    DatabaseWorkload::Learner,
    DatabaseWorkload::Learner,
    DatabaseWorkload::Learner,
    DatabaseWorkload::Learner,
    DatabaseWorkload::Learner,
    DatabaseWorkload::Learner,
    DatabaseWorkload::Learner,
    DatabaseWorkload::Learner,
    DatabaseWorkload::Instructor,
    DatabaseWorkload::Instructor,
    DatabaseWorkload::Instructor,
    DatabaseWorkload::Instructor,
    DatabaseWorkload::Background,
];

type SharedDatabase = Arc<Mutex<Option<Database>>>;
type JobFn = Box<dyn FnOnce(&SharedDatabase) -> JobTiming + Send + 'static>;

struct JobTiming {
    lock_wait: Duration,
    execution: Duration,
}

/// Workload classes for the single-connection database executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatabaseWorkload {
    Learner,
    Instructor,
    Background,
}

impl DatabaseWorkload {
    fn budget(self) -> Duration {
        match self {
            Self::Learner => Duration::from_millis(250),
            Self::Instructor => Duration::from_millis(500),
            Self::Background => Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct QueueCapacities {
    learner: usize,
    instructor: usize,
    background: usize,
}

impl QueueCapacities {
    const PRODUCTION: Self = Self {
        learner: LEARNER_QUEUE_CAPACITY,
        instructor: INSTRUCTOR_QUEUE_CAPACITY,
        background: BACKGROUND_QUEUE_CAPACITY,
    };

    fn for_workload(self, workload: DatabaseWorkload) -> usize {
        match workload {
            DatabaseWorkload::Learner => self.learner,
            DatabaseWorkload::Instructor => self.instructor,
            DatabaseWorkload::Background => self.background,
        }
    }
}

struct Job {
    workload: DatabaseWorkload,
    queued_at: Instant,
    label: &'static str,
    run: JobFn,
}

#[derive(Default)]
struct QueueState {
    learner: VecDeque<Job>,
    instructor: VecDeque<Job>,
    background: VecDeque<Job>,
    schedule_cursor: usize,
    shutting_down: bool,
}

impl QueueState {
    fn len(&self) -> usize {
        self.learner.len() + self.instructor.len() + self.background.len()
    }

    fn push(&mut self, job: Job) {
        match job.workload {
            DatabaseWorkload::Learner => self.learner.push_back(job),
            DatabaseWorkload::Instructor => self.instructor.push_back(job),
            DatabaseWorkload::Background => self.background.push_back(job),
        }
    }

    fn len_for(&self, workload: DatabaseWorkload) -> usize {
        match workload {
            DatabaseWorkload::Learner => self.learner.len(),
            DatabaseWorkload::Instructor => self.instructor.len(),
            DatabaseWorkload::Background => self.background.len(),
        }
    }

    fn pop(&mut self) -> Option<Job> {
        for _ in 0..FAIR_SCHEDULE.len() {
            let workload = FAIR_SCHEDULE[self.schedule_cursor];
            self.schedule_cursor = (self.schedule_cursor + 1) % FAIR_SCHEDULE.len();
            let job = match workload {
                DatabaseWorkload::Learner => self.learner.pop_front(),
                DatabaseWorkload::Instructor => self.instructor.pop_front(),
                DatabaseWorkload::Background => self.background.pop_front(),
            };
            if job.is_some() {
                return job;
            }
        }

        None
    }
}

struct ExecutorInner {
    database: SharedDatabase,
    queue: Mutex<QueueState>,
    ready: Condvar,
    capacities: QueueCapacities,
    clients: AtomicUsize,
}

/// Bounded, priority-aware execution for Alexandria's single SQLite
/// connection. SQL runs on one dedicated thread instead of a Tokio worker.
pub(crate) struct DatabaseExecutor {
    inner: Arc<ExecutorInner>,
}

impl DatabaseExecutor {
    pub(crate) fn new(database: SharedDatabase) -> Self {
        Self::with_capacities(database, QueueCapacities::PRODUCTION)
    }

    #[cfg(test)]
    fn with_capacity(database: SharedDatabase, capacity: usize) -> Self {
        Self::with_capacities(
            database,
            QueueCapacities {
                learner: capacity,
                instructor: capacity,
                background: capacity,
            },
        )
    }

    fn with_capacities(database: SharedDatabase, capacities: QueueCapacities) -> Self {
        assert!(
            capacities.learner > 0 && capacities.instructor > 0 && capacities.background > 0,
            "database executor capacities must be positive"
        );
        let inner = Arc::new(ExecutorInner {
            database,
            queue: Mutex::new(QueueState::default()),
            ready: Condvar::new(),
            capacities,
            clients: AtomicUsize::new(1),
        });
        let worker = Arc::clone(&inner);
        std::thread::Builder::new()
            .name("alexandria-database".into())
            .spawn(move || run_worker(&worker))
            .expect("failed to start database executor");
        Self { inner }
    }

    pub(crate) async fn execute<T, F>(
        &self,
        workload: DatabaseWorkload,
        lease: ProfileLease,
        label: &'static str,
        operation: F,
    ) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        self.execute_inner(workload, Some(lease), label, operation)
            .await
    }

    #[cfg(test)]
    async fn execute_unscoped<T, F>(
        &self,
        workload: DatabaseWorkload,
        label: &'static str,
        operation: F,
    ) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        self.execute_inner(workload, None, label, operation).await
    }

    async fn execute_inner<T, F>(
        &self,
        workload: DatabaseWorkload,
        lease: Option<ProfileLease>,
        label: &'static str,
        operation: F,
    ) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        let (result_tx, result_rx) = oneshot::channel();
        let job = Job {
            workload,
            queued_at: Instant::now(),
            label,
            run: Box::new(move |database| {
                if lease.as_ref().is_some_and(|lease| !lease.is_current()) {
                    let _ = result_tx.send(Err(
                        "profile session changed before database work started".to_string(),
                    ));
                    return JobTiming {
                        lock_wait: Duration::ZERO,
                        execution: Duration::ZERO,
                    };
                }

                let lock_started = Instant::now();
                let guard = database
                    .lock()
                    .map_err(|_| "database lock poisoned".to_string());
                let lock_wait = lock_started.elapsed();
                let execution_started = Instant::now();
                let result = guard.and_then(|guard| {
                    let database = guard.as_ref().ok_or("database not initialized")?;
                    // Contain a panic while the guard is still held: unwinding
                    // through it would poison the shared profile database and
                    // fail every later operation until the app restarts.
                    catch_unwind(AssertUnwindSafe(|| operation(database))).unwrap_or_else(|_| {
                        log::error!("database operation {label} panicked");
                        if !database.conn().is_autocommit() {
                            if let Err(error) = database.conn().execute_batch("ROLLBACK") {
                                log::error!(
                                    "rollback after database operation {label} panicked failed: {error}"
                                );
                            }
                        }
                        Err(format!("database operation {label} panicked"))
                    })
                });
                let execution = execution_started.elapsed();
                let _ = result_tx.send(result);
                JobTiming {
                    lock_wait,
                    execution,
                }
            }),
        };
        self.enqueue(job)?;
        result_rx
            .await
            .map_err(|_| "database executor stopped before completing the operation".to_string())?
    }

    fn enqueue(&self, job: Job) -> Result<(), String> {
        let mut queue = self
            .inner
            .queue
            .lock()
            .map_err(|_| "database executor queue poisoned".to_string())?;
        if queue.shutting_down {
            return Err("database executor is shutting down".to_string());
        }
        if queue.len_for(job.workload) >= self.inner.capacities.for_workload(job.workload) {
            return Err("database is busy; retry the operation".to_string());
        }
        queue.push(job);
        drop(queue);
        self.inner.ready.notify_one();
        Ok(())
    }
}

impl Clone for DatabaseExecutor {
    fn clone(&self) -> Self {
        self.inner.clients.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Drop for DatabaseExecutor {
    fn drop(&mut self) {
        if self.inner.clients.fetch_sub(1, Ordering::AcqRel) == 1 {
            let mut queue = self
                .inner
                .queue
                .lock()
                .expect("database executor queue poisoned");
            queue.shutting_down = true;
            drop(queue);
            self.inner.ready.notify_one();
        }
    }
}

fn run_worker(inner: &Arc<ExecutorInner>) {
    loop {
        let job = {
            let mut queue = inner
                .queue
                .lock()
                .expect("database executor queue poisoned");
            while queue.len() == 0 && !queue.shutting_down {
                queue = inner
                    .ready
                    .wait(queue)
                    .expect("database executor queue poisoned");
            }
            match queue.pop() {
                Some(job) => job,
                None if queue.shutting_down => return,
                None => continue,
            }
        };

        let queue_time = job.queued_at.elapsed();
        let result = catch_unwind(AssertUnwindSafe(|| (job.run)(&inner.database)));
        let total = job.queued_at.elapsed();
        match result {
            Err(_) => log::error!("database operation {} panicked", job.label),
            Ok(timing) if total > job.workload.budget() => {
                log::warn!(
                    "database operation {} exceeded {:?} budget: queue={:?}, lock={:?}, execution={:?}",
                    job.label,
                    job.workload,
                    queue_time,
                    timing.lock_wait,
                    timing.execution,
                );
            }
            Ok(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    fn database() -> SharedDatabase {
        Arc::new(Mutex::new(Some(
            Database::open_in_memory().expect("in-memory database"),
        )))
    }

    async fn wait_until(mut condition: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !condition() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("executor test condition was not reached");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn operation_does_not_block_the_async_runtime() {
        let executor = DatabaseExecutor::with_capacity(database(), 4);
        let started = Arc::new(AtomicBool::new(false));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let started_in_job = Arc::clone(&started);
        let release_in_job = Arc::clone(&release);
        let executor_for_job = executor.clone();
        let job = tokio::spawn(async move {
            executor_for_job
                .execute_unscoped(DatabaseWorkload::Learner, "test.block", move |_| {
                    started_in_job.store(true, Ordering::Release);
                    let (lock, ready) = &*release_in_job;
                    let mut released = lock.lock().expect("release lock");
                    while !*released {
                        released = ready.wait(released).expect("release lock");
                    }
                    Ok(7)
                })
                .await
        });

        wait_until(|| started.load(Ordering::Acquire)).await;
        tokio::time::timeout(Duration::from_millis(100), tokio::task::yield_now())
            .await
            .expect("runtime remained responsive");
        let (lock, ready) = &*release;
        *lock.lock().expect("release lock") = true;
        ready.notify_one();
        assert_eq!(job.await.expect("join").expect("database result"), 7);
    }

    #[tokio::test]
    async fn panicking_operation_reports_error_without_poisoning_database() {
        let shared = database();
        let executor = DatabaseExecutor::with_capacity(Arc::clone(&shared), 4);

        let panicked = executor
            .execute_unscoped(
                DatabaseWorkload::Learner,
                "test.panic",
                |database| -> Result<(), String> {
                    database
                        .conn()
                        .execute_batch("BEGIN IMMEDIATE")
                        .expect("begin transaction");
                    panic!("injected database operation panic")
                },
            )
            .await;
        assert_eq!(
            panicked,
            Err("database operation test.panic panicked".into())
        );
        assert!(!shared.is_poisoned());

        let autocommit = executor
            .execute_unscoped(DatabaseWorkload::Learner, "test.after_panic", |database| {
                Ok(database.conn().is_autocommit())
            })
            .await
            .expect("executor keeps serving after a panicking operation");
        assert!(
            autocommit,
            "open transaction was rolled back after the panic"
        );
    }

    #[tokio::test]
    async fn lane_capacity_reserves_interactive_space() {
        let executor = DatabaseExecutor::with_capacities(
            database(),
            QueueCapacities {
                learner: 1,
                instructor: 1,
                background: 1,
            },
        );
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let started = Arc::new(AtomicBool::new(false));
        let first_release = Arc::clone(&release);
        let first_started = Arc::clone(&started);
        let first_executor = executor.clone();
        let first = tokio::spawn(async move {
            first_executor
                .execute_unscoped(DatabaseWorkload::Learner, "test.first", move |_| {
                    first_started.store(true, Ordering::Release);
                    let (lock, ready) = &*first_release;
                    let mut released = lock.lock().expect("release lock");
                    while !*released {
                        released = ready.wait(released).expect("release lock");
                    }
                    Ok(())
                })
                .await
        });
        wait_until(|| started.load(Ordering::Acquire)).await;

        let second_executor = executor.clone();
        let second = tokio::spawn(async move {
            second_executor
                .execute_unscoped(DatabaseWorkload::Background, "test.second", |_| Ok(()))
                .await
        });
        wait_until(|| executor.inner.queue.lock().expect("queue lock").len() == 1).await;
        let learner_executor = executor.clone();
        let learner = tokio::spawn(async move {
            learner_executor
                .execute_unscoped(DatabaseWorkload::Learner, "test.learner", |_| Ok(()))
                .await
        });
        wait_until(|| executor.inner.queue.lock().expect("queue lock").len() == 2).await;
        let overflow = executor
            .execute_unscoped(DatabaseWorkload::Background, "test.overflow", |_| Ok(()))
            .await;
        assert_eq!(
            overflow,
            Err("database is busy; retry the operation".into())
        );

        let (lock, ready) = &*release;
        *lock.lock().expect("release lock") = true;
        ready.notify_one();
        first.await.expect("first join").expect("first result");
        learner
            .await
            .expect("learner join")
            .expect("learner result");
        second.await.expect("second join").expect("second result");
    }

    #[test]
    fn continuously_backlogged_lanes_follow_eight_four_one_schedule() {
        fn job(workload: DatabaseWorkload) -> Job {
            Job {
                workload,
                queued_at: Instant::now(),
                label: "test.schedule",
                run: Box::new(|_| JobTiming {
                    lock_wait: Duration::ZERO,
                    execution: Duration::ZERO,
                }),
            }
        }

        let mut queue = QueueState::default();
        for _ in 0..16 {
            queue.push(job(DatabaseWorkload::Learner));
        }
        for _ in 0..8 {
            queue.push(job(DatabaseWorkload::Instructor));
        }
        for _ in 0..2 {
            queue.push(job(DatabaseWorkload::Background));
        }

        let actual = (0..26)
            .map(|_| queue.pop().expect("scheduled job").workload)
            .collect::<Vec<_>>();
        let expected = FAIR_SCHEDULE
            .into_iter()
            .chain(FAIR_SCHEDULE)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn learner_work_precedes_queued_background_work() {
        let executor = DatabaseExecutor::with_capacity(database(), 4);
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let order = Arc::new(Mutex::new(Vec::new()));
        let started = Arc::new(AtomicBool::new(false));

        let blocker_release = Arc::clone(&release);
        let blocker_started = Arc::clone(&started);
        let blocker_executor = executor.clone();
        let blocker = tokio::spawn(async move {
            blocker_executor
                .execute_unscoped(DatabaseWorkload::Learner, "test.blocker", move |_| {
                    blocker_started.store(true, Ordering::Release);
                    let (lock, ready) = &*blocker_release;
                    let mut released = lock.lock().expect("release lock");
                    while !*released {
                        released = ready.wait(released).expect("release lock");
                    }
                    Ok(())
                })
                .await
        });
        wait_until(|| started.load(Ordering::Acquire)).await;

        let background_order = Arc::clone(&order);
        let background_executor = executor.clone();
        let background = tokio::spawn(async move {
            background_executor
                .execute_unscoped(DatabaseWorkload::Background, "test.background", move |_| {
                    background_order
                        .lock()
                        .expect("order lock")
                        .push("background");
                    Ok(())
                })
                .await
        });
        wait_until(|| executor.inner.queue.lock().expect("queue lock").len() == 1).await;
        let learner_order = Arc::clone(&order);
        let learner_executor = executor.clone();
        let learner = tokio::spawn(async move {
            learner_executor
                .execute_unscoped(DatabaseWorkload::Learner, "test.learner", move |_| {
                    learner_order.lock().expect("order lock").push("learner");
                    Ok(())
                })
                .await
        });
        wait_until(|| executor.inner.queue.lock().expect("queue lock").len() == 2).await;
        let (lock, ready) = &*release;
        *lock.lock().expect("release lock") = true;
        ready.notify_one();

        blocker
            .await
            .expect("blocker join")
            .expect("blocker result");
        learner
            .await
            .expect("learner join")
            .expect("learner result");
        background
            .await
            .expect("background join")
            .expect("background result");
        assert_eq!(
            *order.lock().expect("order lock"),
            vec!["learner", "background"]
        );
    }

    #[tokio::test]
    async fn queued_profile_work_is_cancelled_after_admission_closes() {
        let executor = DatabaseExecutor::with_capacity(database(), 2);
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let started = Arc::new(AtomicBool::new(false));
        let blocker_release = Arc::clone(&release);
        let blocker_started = Arc::clone(&started);
        let blocker_executor = executor.clone();
        let blocker = tokio::spawn(async move {
            blocker_executor
                .execute_unscoped(DatabaseWorkload::Background, "test.blocker", move |_| {
                    blocker_started.store(true, Ordering::Release);
                    let (lock, ready) = &*blocker_release;
                    let mut released = lock.lock().expect("release lock");
                    while !*released {
                        released = ready.wait(released).expect("release lock");
                    }
                    Ok(())
                })
                .await
        });
        wait_until(|| started.load(Ordering::Acquire)).await;

        let admission = crate::profile::scope::Admission::default();
        let session = admission.close();
        assert!(admission.open(&session));
        let lease = admission.admit(&session).expect("profile lease");
        let ran = Arc::new(AtomicBool::new(false));
        let ran_in_job = Arc::clone(&ran);
        let queued_executor = executor.clone();
        let queued = tokio::spawn(async move {
            queued_executor
                .execute(DatabaseWorkload::Learner, lease, "test.stale", move |_| {
                    ran_in_job.store(true, Ordering::Release);
                    Ok(())
                })
                .await
        });
        wait_until(|| executor.inner.queue.lock().expect("queue lock").len() == 1).await;
        let _ = admission.close();

        let (lock, ready) = &*release;
        *lock.lock().expect("release lock") = true;
        ready.notify_one();
        blocker
            .await
            .expect("blocker join")
            .expect("blocker result");
        assert_eq!(
            queued.await.expect("queued join"),
            Err("profile session changed before database work started".into())
        );
        assert!(!ran.load(Ordering::Acquire));
    }
}
