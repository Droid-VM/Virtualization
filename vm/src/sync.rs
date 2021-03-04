//! Synchronisation utilities.

use std::sync::{Arc, Condvar, Mutex};

/// A flag which one thread can use to notify other threads when a condition becomes true. This is
/// something like a single-use binary semaphore.
#[derive(Clone, Debug)]
pub struct AtomicFlag {
    state: Arc<(Mutex<bool>, Condvar)>,
}

impl Default for AtomicFlag {
    #[allow(clippy::mutex_atomic)]
    fn default() -> Self {
        Self { state: Arc::new((Mutex::new(false), Condvar::new())) }
    }
}

#[allow(clippy::mutex_atomic)]
impl AtomicFlag {
    /// Wait until the flag is set.
    pub fn wait(&self) {
        let _flag = self.state.1.wait_while(self.state.0.lock().unwrap(), |flag| !*flag).unwrap();
    }

    /// Set the flag, and notify all waiting threads.
    pub fn raise(&self) {
        let mut flag = self.state.0.lock().unwrap();
        *flag = true;
        self.state.1.notify_all();
    }
}
