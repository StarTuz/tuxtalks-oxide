use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// A simple robust Circuit Breaker for network-dependent players (`JRiver`).
/// Part of the "Top 1%" reliability standard.
pub struct CircuitBreaker {
    failure_threshold: u32,
    reset_timeout: Duration,
    failures: AtomicU32,
    last_failure: std::sync::Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    #[must_use]
    pub fn new(threshold: u32, timeout_secs: u64) -> Self {
        Self {
            failure_threshold: threshold,
            reset_timeout: Duration::from_secs(timeout_secs),
            failures: AtomicU32::new(0),
            last_failure: std::sync::Mutex::new(None),
        }
    }

    /// Whether the breaker is currently tripped.
    ///
    /// # Panics
    /// Panics if the internal `last_failure` mutex is poisoned (another
    /// thread panicked while holding it).
    #[must_use]
    pub fn is_open(&self) -> bool {
        if self.failures.load(Ordering::SeqCst) >= self.failure_threshold {
            let last = self.last_failure.lock().unwrap();
            if let Some(instant) = *last {
                if instant.elapsed() < self.reset_timeout {
                    return true;
                }
            }
        }
        false
    }

    pub fn record_success(&self) {
        self.failures.store(0, Ordering::SeqCst);
    }

    /// Record a failed call. After `threshold` consecutive failures the
    /// breaker trips until `reset_timeout` elapses.
    ///
    /// # Panics
    /// Panics if the internal `last_failure` mutex is poisoned.
    pub fn record_failure(&self) {
        self.failures.fetch_add(1, Ordering::SeqCst);
        let mut last = self.last_failure.lock().unwrap();
        *last = Some(Instant::now());
    }
}
