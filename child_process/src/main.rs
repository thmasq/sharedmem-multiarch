use linux_futex::{Futex, Shared, TimedWaitError};
use shared_memory::ShmemConf;
use std::env;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

/// The data structure shared between the parent and child processes
/// Must match exactly with the parent's SharedData structure
#[repr(C)]
struct SharedData {
    pub futex: Futex<Shared>,
    pub number: AtomicI64,
}

impl SharedData {
    /// Get the current value of the shared number
    pub fn get_number(&self) -> i64 {
        self.number.load(Ordering::SeqCst)
    }

    /// Set the shared number to a new value
    pub fn set_number(&self, value: i64) {
        self.number.store(value, Ordering::SeqCst);
    }

    /// Acquire the futex lock with a timeout
    pub fn lock_timeout(&self, timeout: Duration) -> Result<(), TimedWaitError> {
        let start = std::time::Instant::now();

        loop {
            // Check timeout
            if start.elapsed() >= timeout {
                return Err(TimedWaitError::TimedOut);
            }

            // Try to change futex value from 0 (unlocked) to 1 (locked)
            match self
                .futex
                .value
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => return Ok(()), // Successfully acquired lock
                Err(_) => {
                    // Lock is contended, wait for it to be released with remaining timeout
                    let remaining = timeout.saturating_sub(start.elapsed());
                    if remaining.is_zero() {
                        return Err(TimedWaitError::TimedOut);
                    }
                    let _ = self.futex.wait_for(1, remaining)?;
                }
            }
        }
    }

    /// Release the futex lock and wake up waiting processes
    pub fn unlock(&self) {
        self.futex.value.store(0, Ordering::Release);
        self.futex.wake(1); // Wake up one waiting process
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 32-bit Child Process Started ===");
    println!("Child Process ID: {}", std::process::id());

    // Get shared memory OS ID from command line argument
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        return Err("Usage: child_process <shared_memory_os_id>".into());
    }

    let os_id = &args[1];
    println!("Child: Opening shared memory with OS ID: {}", os_id);

    // Open the existing shared memory using the OS ID
    let shmem = ShmemConf::new().os_id(os_id).open()?;
    println!("Child: Successfully opened shared memory");

    // Get a pointer to the shared data
    let shared_data_ptr = shmem.as_ptr() as *const SharedData;
    let shared_data = unsafe { &*shared_data_ptr };

    // Verify we can read the shared data
    let initial_number = shared_data.get_number();
    println!("Child: Can see initial number: {}", initial_number);

    if initial_number != 100 {
        eprintln!(
            "Child: Warning - expected initial number 100, got {}",
            initial_number
        );
    }

    // Attempt to acquire the lock (will block until parent releases it)
    println!("Child: Attempting to acquire lock...");

    // Use a timeout to avoid hanging indefinitely
    const TIMEOUT_SECONDS: u64 = 10;
    let timeout = Duration::from_secs(TIMEOUT_SECONDS);

    match shared_data.lock_timeout(timeout) {
        Ok(_) => {
            println!("Child: Lock acquired successfully!");
        }
        Err(TimedWaitError::TimedOut) => {
            return Err(format!(
                "Child: Timeout waiting for lock after {} seconds",
                TIMEOUT_SECONDS
            )
            .into());
        }
        Err(TimedWaitError::Interrupted) => {
            return Err("Child: Lock attempt was interrupted".into());
        }
        Err(e) => {
            return Err(format!("Child: Failed to acquire lock: {:?}", e).into());
        }
    }

    // Read the current number
    let current_number = shared_data.get_number();
    println!("Child: Current number: {}", current_number);

    // Child does math: add 25 and multiply by 2
    let new_number = (current_number + 25) * 2;
    shared_data.set_number(new_number);

    println!("Child: Applied operation ((n + 25) * 2)");
    println!("Child: New number: {}", new_number);

    // Simulate some work
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Release the lock
    shared_data.unlock();
    println!("Child: Lock released");

    println!("=== Child process finished successfully ===");

    Ok(())
}
