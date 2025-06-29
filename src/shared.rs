use linux_futex::{Futex, Shared, TimedWaitError, WaitError};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

#[repr(C)]
pub struct SharedData {
    pub futex: Futex<Shared>,
    pub number: AtomicI64,
}

#[allow(dead_code)]
impl SharedData {
    pub fn new() -> Self {
        Self {
            futex: Futex::new(0),
            number: AtomicI64::new(100),
        }
    }

    pub fn get_number(&self) -> i64 {
        self.number.load(Ordering::SeqCst)
    }

    pub fn set_number(&self, value: i64) {
        self.number.store(value, Ordering::SeqCst);
    }

    pub fn lock(&self) -> Result<(), WaitError> {
        loop {
            match self
                .futex
                .value
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => return Ok(()),
                Err(_) => {
                    let _ = self.futex.wait(1)?;
                }
            }
        }
    }

    pub fn lock_timeout(&self, timeout: Duration) -> Result<(), TimedWaitError> {
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() >= timeout {
                return Err(TimedWaitError::TimedOut);
            }

            match self
                .futex
                .value
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => return Ok(()),
                Err(_) => {
                    let remaining = timeout.saturating_sub(start.elapsed());
                    if remaining.is_zero() {
                        return Err(TimedWaitError::TimedOut);
                    }
                    let _ = self.futex.wait_for(1, remaining)?;
                }
            }
        }
    }

    pub fn unlock(&self) {
        self.futex.value.store(0, Ordering::Release);
        self.futex.wake(1);
    }

    pub fn try_lock(&self) -> bool {
        self.futex
            .value
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }
}
