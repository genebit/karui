//! What the app is currently doing.

use crate::error::{AppError, Result};
use karui_core::devices::Ledger;
use karui_core::estimate::{Rate, Rates};
use karui_core::options::CompressOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant};

/// The batch in flight, if any, held as its cancel flag.
///
/// One batch at a time: two would compete for the same cores and finish no
/// sooner, and a second could plan an output the first is about to write.
#[derive(Default)]
pub struct Runner {
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

impl Runner {
    /// Claim the runner for a new batch.
    pub fn begin(&self) -> Result<Arc<AtomicBool>> {
        let mut slot = self.cancel.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Err(AppError::Busy);
        }
        let flag = Arc::new(AtomicBool::new(false));
        *slot = Some(flag.clone());
        Ok(flag)
    }

    /// Ask the running batch to stop. `false` if nothing was running.
    pub fn cancel(&self) -> bool {
        match &*self.cancel.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    pub fn finish(&self) {
        *self.cancel.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    pub fn running(&self) -> bool {
        self.cancel
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Cancel and give the batch up to `grace` to kill ffmpeg and remove its
    /// working file.
    pub fn cancel_and_wait(&self, grace: Duration) {
        if !self.cancel() {
            return;
        }
        let deadline = Instant::now() + grace;
        while self.running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Short encodes the window asks for outside a batch: preview samples and
/// speed benchmarks.
///
/// Only the latest of each kind matters. Choosing another file or moving the
/// slider makes the previous preview useless, and changing the preset makes
/// the previous benchmark useless, so starting one cancels the last of its
/// kind. Both kinds take turns, since two encodes at once only split the
/// machine and would time each other wrongly.
#[derive(Default)]
pub struct Sidework {
    preview: Mutex<Option<Arc<AtomicBool>>>,
    benchmark: Mutex<Option<Arc<AtomicBool>>>,
    sizing: Mutex<Option<Arc<AtomicBool>>>,
    turn: Mutex<()>,
}

#[derive(Clone, Copy)]
pub enum Task {
    Preview,
    Benchmark,
    /// Size estimates, which run through the whole list in the background.
    Sizing,
}

impl Sidework {
    fn slot(&self, task: Task) -> &Mutex<Option<Arc<AtomicBool>>> {
        match task {
            Task::Preview => &self.preview,
            Task::Benchmark => &self.benchmark,
            Task::Sizing => &self.sizing,
        }
    }

    /// Cancel the previous task of this kind and claim a flag for the next.
    pub fn begin(&self, task: Task) -> Arc<AtomicBool> {
        // A preview is what the user is looking at; a size estimate is
        // background work that is simply asked for again.
        if matches!(task, Task::Preview) {
            self.stop(Task::Sizing);
        }
        let mut slot = self
            .slot(task)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = slot.take() {
            previous.store(true, Ordering::Relaxed);
        }
        let flag = Arc::new(AtomicBool::new(false));
        *slot = Some(flag.clone());
        flag
    }

    /// Stop the task of this kind in flight, if any.
    fn stop(&self, task: Task) {
        if let Some(flag) = self
            .slot(task)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Stop everything in flight. A batch calls this before it starts.
    pub fn cancel(&self) {
        for task in [Task::Preview, Task::Benchmark, Task::Sizing] {
            self.stop(task);
        }
    }

    /// Cancel and give the running task up to `grace` to kill its encode.
    pub fn cancel_and_wait(&self, grace: Duration) {
        self.cancel();
        let deadline = Instant::now() + grace;
        while matches!(self.turn.try_lock(), Err(TryLockError::WouldBlock))
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Held for the length of one task.
    pub fn turn(&self) -> MutexGuard<'_, ()> {
        self.turn.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Encode rates for estimates, saved between launches.
pub struct RateStore {
    /// `None` when the app has no data directory; rates then last a session.
    path: Option<PathBuf>,
    rates: Mutex<Rates>,
}

impl RateStore {
    pub fn load(path: Option<PathBuf>) -> Self {
        let rates = path.as_deref().map(Rates::load).unwrap_or_default();
        Self {
            path,
            rates: Mutex::new(rates),
        }
    }

    pub fn get(&self, opts: &CompressOptions) -> Option<Rate> {
        self.lock().get(opts)
    }

    /// Change the rates and save them. A failed save costs a benchmark next
    /// launch, nothing more.
    pub fn update(&self, change: impl FnOnce(&mut Rates)) {
        let mut rates = self.lock();
        change(&mut rates);
        if let Some(path) = &self.path {
            if let Err(e) = rates.save(path) {
                tracing::debug!("could not save encode rates: {e}");
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, Rates> {
        self.rates.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Card videos already imported, saved between launches so a card put back
/// in after more shooting offers only the new clips.
pub struct CardLedger {
    /// `None` when the app has no data directory; the ledger then lasts a
    /// session.
    path: Option<PathBuf>,
    ledger: Mutex<Ledger>,
}

impl CardLedger {
    pub fn load(path: Option<PathBuf>) -> Self {
        let ledger = path.as_deref().map(Ledger::load).unwrap_or_default();
        Self {
            path,
            ledger: Mutex::new(ledger),
        }
    }

    pub fn snapshot(&self) -> Ledger {
        self.ledger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn record(&self, input: &Path) {
        let mut ledger = self.ledger.lock().unwrap_or_else(PoisonError::into_inner);
        if !ledger.record(input) {
            return;
        }
        if let Some(path) = &self.path {
            if let Err(e) = ledger.save(path) {
                tracing::debug!("could not save the import ledger: {e}");
            }
        }
    }
}

/// Releases the runner when a batch thread ends, panicking or not, so one bad
/// batch cannot leave the app permanently "busy".
pub struct RunGuard(pub Arc<Runner>);

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// Paths given on the command line, handed to the window once it mounts.
#[derive(Default)]
pub struct LaunchPaths(Mutex<Vec<String>>);

impl LaunchPaths {
    pub fn from_args() -> Self {
        let paths = std::env::args_os()
            .skip(1)
            .map(std::path::PathBuf::from)
            .filter(|p| p.exists())
            .map(|p| p.display().to_string())
            .collect();
        Self(Mutex::new(paths))
    }

    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}
