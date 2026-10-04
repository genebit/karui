//! What the app is currently doing.

use crate::error::{AppError, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
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

    fn running(&self) -> bool {
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
