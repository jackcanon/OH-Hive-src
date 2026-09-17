//! Keeps a node doing what its member asked, and says so honestly when it cannot.
//!
//! # Why this exists
//!
//! On 2026-09-17 Midgaard sat checked out for twelve hours with the Hive app running and
//! `HIVE_WORKER_ENABLED=1` on disk. Nothing was broken in any single place. There were simply two
//! facts about whether the machine was working -- a persisted setting and a live task -- and
//! nobody owned the gap between them:
//!
//! * **Intent** lives in the member's config. It survives restarts and only they change it.
//! * **Actual** was `worker_stop.is_some()` in the FFI: whether a loop is running this instant,
//!   memory-only, gone the moment the loop ends for any reason at all.
//!
//! The app painted both as one status, so "you switched it off" and "the loop died an hour ago"
//! rendered identically, and the only route back was a human noticing and flipping a toggle.
//!
//! A supervisor is the thing whose whole job is to drive actual toward intent, and to be honest
//! when it can't. It owns three jobs the old code had nowhere to put: resuming at launch,
//! restarting after an end nobody asked for, and reporting a state other than on/off.
//!
//! # The property that makes it robust
//!
//! It never trusts *why* a run ended more than it trusts what the member asked for. If a run stops
//! while intent still says work -- however it stopped, whatever reason it reported, including a
//! clean-looking [`WorkerExit::Requested`] nobody in this process requested -- it starts again.
//! That matters because the original trigger on Midgaard was never identified. A supervisor that
//! only handled the failures we had diagnosed would not have helped.
//!
//! # Idle is not absent
//!
//! While not working, the supervisor keeps pinging the hub. A machine sitting there healthy but
//! checked out is idle, not gone, and until migration `20260917150000` the fleet had no way to say
//! so: liveness now lands in `hive.nodes.last_seen`, availability stays in `last_heartbeat`, and
//! the reapers keep reading availability. See that migration's header for why widening the old
//! column instead would have quietly disarmed the orphaned-lease reaper.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
// `tokio::time::Instant`, not `std::time::Instant`: every deadline here is compared against time
// this task sleeps through, and tokio's clock is the one that sleeping actually advances. With the
// std type the idle-wait deadline never arrives under a paused clock -- `remaining` recomputes to
// the same five seconds on every pass and the loop sleeps forever. That was a test-only symptom of
// a real category error: a deadline in async code belongs to the runtime's clock.
use tokio::time::Instant;

pub use crate::worker::WorkerExit;

/// What the member asked for. Persisted; changed only by them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Take work.
    Work,
    /// Don't.
    Rest,
}

/// What a member should be shown.
///
/// The first two are the states the old UI had. The last two are the ones it lacked, which is why
/// a node that had died looked exactly like a node someone had switched off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStatus {
    /// Working is switched off. Nothing is wrong.
    Resting,
    /// Taking work.
    Working,
    /// A run ended without being asked to, and another attempt is scheduled.
    Retrying {
        reason: String,
        attempt: u32,
        next_attempt_in: Duration,
    },
    /// Stopped for something another attempt cannot fix. This one needs the member.
    Blocked { reason: String },
}

/// How long to wait before each successive restart, and when to forgive.
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    /// Waits for attempts 1, 2, 3, then `cap` for every attempt after.
    pub steps: [Duration; 3],
    pub cap: Duration,
    /// A run that lasted at least this long counts as healthy and resets the ladder, so a node
    /// that works fine for hours and hiccups once does not inherit yesterday's five-minute wait.
    pub healthy_after: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        // Deliberately not aggressive. The most likely reason several nodes restart at once is the
        // hub having a bad minute, and a fleet retrying every five seconds is how a small outage
        // becomes a large one.
        Self {
            steps: [
                Duration::from_secs(5),
                Duration::from_secs(15),
                Duration::from_secs(60),
            ],
            cap: Duration::from_secs(300),
            healthy_after: Duration::from_secs(600),
        }
    }
}

impl Backoff {
    /// `attempt` counts from 1.
    pub fn wait(&self, attempt: u32) -> Duration {
        *self
            .steps
            .get(attempt.saturating_sub(1) as usize)
            .unwrap_or(&self.cap)
    }
}

/// The worker, as the supervisor needs to see it.
///
/// Implemented where the worker is actually built -- the FFI, the CLI -- because assembling one
/// needs a hub client, a backend, capabilities and a sandbox, none of which belong in here.
#[async_trait::async_trait]
pub trait SupervisedWorker: Send + Sync + 'static {
    /// Run until `stop` flips, reporting why the run ended.
    async fn run(&self, stop: watch::Receiver<bool>) -> anyhow::Result<WorkerExit>;

    /// Report that this machine is alive while it is not taking work.
    ///
    /// Failure is not interesting: the hub being unreachable for one ping is ordinary, and the
    /// next one is a minute away.
    async fn ping(&self);

    /// True for causes another attempt cannot fix -- not paired, key revoked, no models.
    ///
    /// Restarting those in a loop burns the member's battery and buries the message that would
    /// have told them what to do. Default is `false`: something unrecognised is worth retrying,
    /// because the failure we have not seen yet is more often transient than terminal.
    fn is_terminal(&self, _error: &anyhow::Error) -> bool {
        false
    }
}

/// Drives actual state toward intent. See the module doc.
pub struct Supervisor<W: SupervisedWorker> {
    worker: Arc<W>,
    intent: watch::Receiver<Intent>,
    status: watch::Sender<WorkerStatus>,
    backoff: Backoff,
    idle_ping: Duration,
}

impl<W: SupervisedWorker> Supervisor<W> {
    /// `intent` is the member's setting, watched live so a toggle takes effect without a restart.
    /// The returned receiver is what a UI renders.
    pub fn new(
        worker: Arc<W>,
        intent: watch::Receiver<Intent>,
    ) -> (Self, watch::Receiver<WorkerStatus>) {
        let (status, rx) = watch::channel(WorkerStatus::Resting);
        (
            Self {
                worker,
                intent,
                status,
                backoff: Backoff::default(),
                idle_ping: Duration::from_secs(60),
            },
            rx,
        )
    }

    pub fn with_backoff(mut self, backoff: Backoff) -> Self {
        self.backoff = backoff;
        self
    }

    pub fn with_idle_ping(mut self, every: Duration) -> Self {
        self.idle_ping = every;
        self
    }

    fn publish(&self, status: WorkerStatus) {
        // A log line per transition, always. A supervisor that recovers quietly turns a bug into a
        // mystery -- which is how a node spent twelve hours doing nothing behind a log line that
        // read like a deliberate shutdown.
        match &status {
            WorkerStatus::Resting => tracing::info!("worker resting (switched off)"),
            WorkerStatus::Working => tracing::info!("worker working"),
            WorkerStatus::Retrying {
                reason,
                attempt,
                next_attempt_in,
            } => tracing::warn!(
                attempt,
                "worker stopped without being asked ({reason}); retrying in {}s",
                next_attempt_in.as_secs()
            ),
            WorkerStatus::Blocked { reason } => {
                tracing::error!("worker blocked, a restart will not fix this: {reason}")
            }
        }
        let _ = self.status.send(status);
    }

    /// Runs until the intent channel's sender is dropped, i.e. until the app is going away.
    pub async fn run(mut self) {
        let mut attempt: u32 = 0;
        loop {
            if *self.intent.borrow() == Intent::Rest {
                self.publish(WorkerStatus::Resting);
                attempt = 0;
                if self.idle_until_intent_changes().await.is_none() {
                    return;
                }
                continue;
            }

            self.publish(WorkerStatus::Working);
            let started = Instant::now();
            let outcome = self.run_once().await;
            if started.elapsed() >= self.backoff.healthy_after {
                attempt = 0;
            }

            // The member asked for rest while that run was in flight: the run ending is the
            // expected consequence, not something to recover from.
            if *self.intent.borrow() == Intent::Rest {
                continue;
            }

            let reason = match outcome {
                // Intent still says work, so a stop that reports itself as requested was not
                // requested by anyone we know about. This is the Midgaard case, and it is handled
                // without ever having identified what fired it.
                Ok(WorkerExit::Requested) => "stopped by something other than your setting".into(),
                Ok(WorkerExit::Abandoned) => "the worker's stop channel was dropped".into(),
                Err(error) => {
                    if self.worker.is_terminal(&error) {
                        self.publish(WorkerStatus::Blocked {
                            reason: error.to_string(),
                        });
                        if self.idle_until_intent_changes().await.is_none() {
                            return;
                        }
                        attempt = 0;
                        continue;
                    }
                    error.to_string()
                }
            };

            attempt = attempt.saturating_add(1);
            let wait = self.backoff.wait(attempt);
            self.publish(WorkerStatus::Retrying {
                reason,
                attempt,
                next_attempt_in: wait,
            });
            // Still pinging while we wait: retrying is a live machine, and a fleet view that showed
            // it as absent would be repeating the exact error this whole change is about.
            if self.idle_for(wait).await.is_none() {
                return;
            }
        }
    }

    /// One run, ended either by the worker itself or by the member switching working off.
    ///
    /// Takes `&mut self` for the same reason the two idle helpers do: a `watch::Receiver` marks a
    /// change seen on the receiver that observed it. An earlier draft cloned `self.intent` in each
    /// helper, so the clone consumed the notification, was dropped, and the supervisor's own
    /// receiver still had it pending -- every subsequent clone then returned immediately and the
    /// loop span. It span *hard*: a blocked worker restarted 244 times in a test that expected one.
    /// Sharing one receiver is what makes "wait until the member changes their mind" actually wait.
    async fn run_once(&mut self) -> anyhow::Result<WorkerExit> {
        // The supervisor holds the sender for the whole run. That alone retires the original
        // `Abandoned` path in this process: the channel cannot be dropped out from under a worker
        // that is still running, because the thing holding it outlives the run by construction.
        let (stop_tx, stop_rx) = watch::channel(false);
        let worker = self.worker.clone();
        let mut handle = tokio::spawn(async move { worker.run(stop_rx).await });
        loop {
            tokio::select! {
                joined = &mut handle => {
                    return match joined {
                        Ok(result) => result,
                        // A panic inside the worker is not a reason to leave the node retired.
                        Err(e) => Err(anyhow::anyhow!("worker task ended abnormally: {e}")),
                    };
                }
                changed = self.intent.changed() => {
                    // Sender dropped: the app is going away. Ask the worker to check out and wait
                    // for it, so we leave the fleet cleanly rather than abandoning a lease.
                    let rest = changed.is_err() || *self.intent.borrow() == Intent::Rest;
                    if rest {
                        let _ = stop_tx.send(true);
                    }
                }
            }
        }
    }

    /// Ping on a slow timer until the member changes their mind. `None` once intent is gone.
    async fn idle_until_intent_changes(&mut self) -> Option<()> {
        loop {
            tokio::select! {
                changed = self.intent.changed() => return changed.ok(),
                _ = tokio::time::sleep(self.idle_ping) => self.worker.ping().await,
            }
        }
    }

    /// Ping on a slow timer for `total`, returning early if intent changes. `None` once intent is
    /// gone.
    async fn idle_for(&mut self, total: Duration) -> Option<()> {
        let deadline = Instant::now() + total;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Some(());
            }
            tokio::select! {
                changed = self.intent.changed() => return changed.ok(),
                _ = tokio::time::sleep(remaining.min(self.idle_ping)) => {
                    if remaining > self.idle_ping {
                        self.worker.ping().await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A worker that does whatever the test tells it to, and counts.
    struct Fake {
        runs: AtomicU32,
        pings: AtomicU32,
        /// One outcome per run, popped in order; the last one repeats forever.
        script: std::sync::Mutex<Vec<Outcome>>,
    }
    #[derive(Clone)]
    enum Outcome {
        /// Ends immediately with this exit.
        Ends(WorkerExit),
        /// Ends immediately with an error.
        Errors(&'static str),
        /// Runs until asked to stop, then reports a requested exit -- a healthy worker.
        Serves,
    }

    impl Fake {
        fn new(script: Vec<Outcome>) -> Arc<Self> {
            Arc::new(Self {
                runs: AtomicU32::new(0),
                pings: AtomicU32::new(0),
                script: std::sync::Mutex::new(script),
            })
        }
        fn runs(&self) -> u32 {
            self.runs.load(Ordering::SeqCst)
        }
        fn pings(&self) -> u32 {
            self.pings.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl SupervisedWorker for Fake {
        async fn run(&self, mut stop: watch::Receiver<bool>) -> anyhow::Result<WorkerExit> {
            let n = self.runs.fetch_add(1, Ordering::SeqCst) as usize;
            let script = self.script.lock().unwrap().clone();
            let outcome = script
                .get(n)
                .or_else(|| script.last())
                .cloned()
                .unwrap_or(Outcome::Serves);
            match outcome {
                Outcome::Ends(exit) => Ok(exit),
                Outcome::Errors(message) => Err(anyhow::anyhow!(message)),
                Outcome::Serves => {
                    while !*stop.borrow() {
                        if stop.changed().await.is_err() {
                            return Ok(WorkerExit::Abandoned);
                        }
                    }
                    Ok(WorkerExit::Requested)
                }
            }
        }
        async fn ping(&self) {
            self.pings.fetch_add(1, Ordering::SeqCst);
        }
        fn is_terminal(&self, error: &anyhow::Error) -> bool {
            error.to_string().contains("pair this machine")
        }
    }

    fn spawn(
        worker: Arc<Fake>,
        intent: Intent,
    ) -> (
        watch::Sender<Intent>,
        watch::Receiver<WorkerStatus>,
        tokio::task::JoinHandle<()>,
    ) {
        let (tx, rx) = watch::channel(intent);
        let (sup, status) = Supervisor::new(worker, rx);
        let sup = sup
            .with_backoff(Backoff {
                steps: [
                    Duration::from_secs(5),
                    Duration::from_secs(15),
                    Duration::from_secs(60),
                ],
                cap: Duration::from_secs(300),
                healthy_after: Duration::from_secs(600),
            })
            .with_idle_ping(Duration::from_secs(60));
        (tx, status, tokio::spawn(sup.run()))
    }

    /// Let spawned tasks actually make progress under paused time.
    async fn settle() {
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }

    /// Wait for a status the supervisor publishes, rather than advancing the clock by hand and
    /// hoping the right task got polled in between.
    ///
    /// Under `start_paused` tokio auto-advances to the next timer whenever the runtime goes idle,
    /// so the backoff waits and idle pings elapse on their own. Asserting on the published status
    /// says what these tests actually care about -- the supervisor's decisions -- instead of
    /// encoding a guess about scheduler interleaving.
    async fn wait_for(
        status: &mut watch::Receiver<WorkerStatus>,
        what: &str,
        pred: impl Fn(&WorkerStatus) -> bool,
    ) -> WorkerStatus {
        let found = tokio::time::timeout(Duration::from_secs(86_400), async {
            loop {
                {
                    let current = status.borrow_and_update().clone();
                    if pred(&current) {
                        return current;
                    }
                }
                if status.changed().await.is_err() {
                    panic!("the supervisor stopped while waiting for {what}");
                }
            }
        })
        .await;
        found.unwrap_or_else(|_| panic!("timed out waiting for {what}"))
    }

    fn retrying(attempt: u32) -> impl Fn(&WorkerStatus) -> bool {
        move |s| matches!(s, WorkerStatus::Retrying { attempt: a, .. } if *a == attempt)
    }

    /// The launch case. Intent says work, so something starts working -- without anyone touching a
    /// toggle. This is the whole of what the Swift app was missing.
    #[tokio::test(start_paused = true)]
    async fn starts_working_when_intent_says_work() {
        let w = Fake::new(vec![Outcome::Serves]);
        let (_tx, status, _h) = spawn(w.clone(), Intent::Work);
        settle().await;
        assert_eq!(w.runs(), 1);
        assert_eq!(*status.borrow(), WorkerStatus::Working);
    }

    /// THE MIDGAARD CASE. A run ends reporting a perfectly ordinary requested stop, but the member
    /// never asked for one -- intent still says work. The supervisor does not care what the exit
    /// claimed; it compares against intent and starts again. This passes without anyone ever having
    /// identified what fired the original stop, which is the point.
    #[tokio::test(start_paused = true)]
    async fn restarts_a_stop_the_member_never_asked_for() {
        let w = Fake::new(vec![Outcome::Ends(WorkerExit::Requested), Outcome::Serves]);
        let (_tx, mut status, _h) = spawn(w.clone(), Intent::Work);
        let s = wait_for(&mut status, "the first retry", retrying(1)).await;
        match s {
            WorkerStatus::Retrying { reason, .. } => assert!(
                reason.contains("other than your setting"),
                "the reason should name the thing that is actually odd here, got {reason:?}"
            ),
            other => panic!("expected Retrying, got {other:?}"),
        };
        wait_for(&mut status, "the restart", |s| *s == WorkerStatus::Working).await;
        assert_eq!(w.runs(), 2, "the second run never started");
    }

    /// The other half of the same judgement: when the member DOES switch working off, the run ends
    /// and stays ended. A supervisor that fought the toggle would be worse than no supervisor.
    #[tokio::test(start_paused = true)]
    async fn honours_the_member_switching_working_off() {
        let w = Fake::new(vec![Outcome::Serves]);
        let (tx, status, _h) = spawn(w.clone(), Intent::Work);
        settle().await;
        assert_eq!(w.runs(), 1);
        tx.send(Intent::Rest).unwrap();
        settle().await;
        assert_eq!(*status.borrow(), WorkerStatus::Resting);
        tokio::time::advance(Duration::from_secs(600)).await;
        settle().await;
        assert_eq!(w.runs(), 1, "rest must not be restarted into work");
    }

    /// A cause another attempt cannot fix stops and says so, instead of looping forever and burying
    /// the one message that tells the member what to do.
    #[tokio::test(start_paused = true)]
    async fn a_terminal_cause_blocks_rather_than_retrying() {
        let w = Fake::new(vec![Outcome::Errors("pair this machine first")]);
        let (tx, status, _h) = spawn(w.clone(), Intent::Work);
        settle().await;
        assert!(matches!(&*status.borrow(), WorkerStatus::Blocked { .. }));
        tokio::time::advance(Duration::from_secs(3600)).await;
        settle().await;
        assert_eq!(w.runs(), 1, "a blocked worker must not spin");

        // And it is not a dead end: the member fixing it and toggling is a fresh start.
        tx.send(Intent::Rest).unwrap();
        settle().await;
        tx.send(Intent::Work).unwrap();
        settle().await;
        assert_eq!(w.runs(), 2);
    }

    /// The ladder lengthens, so a fleet whose hub is having a bad minute does not become the reason
    /// the minute gets worse.
    #[tokio::test(start_paused = true)]
    async fn backoff_lengthens_between_attempts() {
        let w = Fake::new(vec![Outcome::Errors("hub unreachable")]);
        let (_tx, mut status, _h) = spawn(w.clone(), Intent::Work);
        for (attempt, wait) in [(1u32, 5u64), (2, 15), (3, 60), (4, 300), (5, 300)] {
            match wait_for(&mut status, "a retry", retrying(attempt)).await {
                WorkerStatus::Retrying {
                    next_attempt_in, ..
                } => assert_eq!(
                    next_attempt_in.as_secs(),
                    wait,
                    "attempt {attempt} waited the wrong amount"
                ),
                other => panic!("expected Retrying, got {other:?}"),
            };
        }
    }

    /// A node that worked fine for hours and hiccups once should not inherit yesterday's five
    /// minute wait.
    #[tokio::test(start_paused = true)]
    async fn a_healthy_run_forgives_the_ladder() {
        let w = Fake::new(vec![
            Outcome::Errors("blip"),
            Outcome::Serves,
            Outcome::Errors("blip"),
            Outcome::Serves,
        ]);
        let (tx, mut status, _h) = spawn(w.clone(), Intent::Work);
        wait_for(&mut status, "the first retry", retrying(1)).await;
        wait_for(&mut status, "the restart", |s| *s == WorkerStatus::Working).await;

        // A serving worker parks on its stop channel and holds no timer, so nothing auto-advances
        // here -- this is the one place the clock has to be pushed by hand, and it is deterministic
        // precisely because there is nothing else pending.
        tokio::time::advance(Duration::from_secs(660)).await;
        settle().await;

        // Knock it over again. Eleven minutes of healthy work should have forgiven the ladder, so
        // this reads as a first failure rather than inheriting yesterday's wait.
        tx.send(Intent::Rest).unwrap();
        wait_for(&mut status, "rest", |s| *s == WorkerStatus::Resting).await;
        tx.send(Intent::Work).unwrap();
        match wait_for(&mut status, "a retry", |s| {
            matches!(s, WorkerStatus::Retrying { .. })
        })
        .await
        {
            WorkerStatus::Retrying {
                attempt,
                next_attempt_in,
                ..
            } => {
                assert_eq!(
                    attempt, 1,
                    "the ladder should have reset after a healthy run"
                );
                assert_eq!(next_attempt_in.as_secs(), 5);
            }
            other => panic!("expected Retrying, got {other:?}"),
        };
    }

    /// Idle hands report as idle. Without this the fleet reads a machine that is sitting there
    /// perfectly healthy as one that was unplugged.
    #[tokio::test(start_paused = true)]
    async fn an_idle_node_keeps_saying_it_is_alive() {
        let w = Fake::new(vec![Outcome::Serves]);
        let (_tx, _status, _h) = spawn(w.clone(), Intent::Rest);
        settle().await;
        assert_eq!(w.pings(), 0);
        for expected in 1..=3 {
            tokio::time::advance(Duration::from_secs(61)).await;
            settle().await;
            assert_eq!(
                w.pings(),
                expected,
                "an idle node stopped reporting liveness"
            );
        }
        assert_eq!(w.runs(), 0, "resting must not take work");
    }

    /// Retrying is a live machine too, so it keeps reporting. Showing it as absent would repeat the
    /// exact error this change is about.
    #[tokio::test(start_paused = true)]
    async fn a_retrying_node_keeps_saying_it_is_alive() {
        let w = Fake::new(vec![Outcome::Errors("blip")]);
        let (_tx, mut status, _h) = spawn(w.clone(), Intent::Work);
        // Climb to the five-minute wait, which is long enough to contain several one-minute pings.
        wait_for(&mut status, "the fourth retry", retrying(4)).await;
        let before = w.pings();
        wait_for(&mut status, "the fifth retry", retrying(5)).await;
        assert!(
            w.pings() >= before + 4,
            "a node waiting out a five minute backoff must keep reporting liveness; \
             pings went {before} -> {}",
            w.pings()
        );
    }

    #[test]
    fn backoff_caps_rather_than_growing_without_bound() {
        let b = Backoff::default();
        assert_eq!(b.wait(1), Duration::from_secs(5));
        assert_eq!(b.wait(4), b.cap);
        assert_eq!(b.wait(u32::MAX), b.cap);
        // attempt 0 should not panic or index out of bounds even though callers count from 1.
        assert_eq!(b.wait(0), Duration::from_secs(5));
    }
}
