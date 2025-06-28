use std::env;
use std::ffi::CString;
use std::ptr;
use std::sync::atomic;

// Use the same layout as parent
const MUTEX_OFFSET: usize = 0;
const NUMBER_OFFSET: usize = 256;
const SHARED_MEMORY_SIZE: usize = 4096;

unsafe fn get_mutex_ptr(shared_ptr: *mut u8) -> *mut libc::pthread_mutex_t {
    shared_ptr.add(MUTEX_OFFSET) as *mut libc::pthread_mutex_t
}

unsafe fn get_number_ptr(shared_ptr: *mut u8) -> *mut i64 {
    shared_ptr.add(NUMBER_OFFSET) as *mut i64
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 32-bit Child Process Started ===");
    println!("Child Process ID: {}", std::process::id());

    // Get shared memory name from command line argument
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        return Err("Usage: child_process <shared_memory_name>".into());
    }

    let shm_name = CString::new(args[1].as_str())?;

    // Open existing shared memory
    let shm_fd = unsafe { libc::shm_open(shm_name.as_ptr(), libc::O_RDWR, 0) };

    if shm_fd == -1 {
        return Err("Child: Failed to open shared memory".into());
    }

    // Map the shared memory
    let shared_ptr = unsafe {
        libc::mmap(
            ptr::null_mut(),
            SHARED_MEMORY_SIZE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            shm_fd,
            0,
        )
    };

    if shared_ptr == libc::MAP_FAILED {
        unsafe {
            libc::close(shm_fd);
        }
        return Err("Child: Failed to map shared memory".into());
    }

    let shared_base = shared_ptr as *mut u8;
    let mutex_ptr = unsafe { get_mutex_ptr(shared_base) };
    let number_ptr = unsafe { get_number_ptr(shared_base) };

    println!("Child: Shared memory mapped successfully");

    // Verify we can read the shared data
    unsafe {
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        let initial_number = *number_ptr;
        println!("Child: Can see initial number: {}", initial_number);
        if initial_number != 100 {
            eprintln!(
                "Child: Warning - expected initial number 100, got {}",
                initial_number
            );
        }
    }

    // Try to acquire the mutex with timeout (will block until parent releases it)
    unsafe {
        println!("Child: Attempting to acquire mutex...");

        // Try to acquire the mutex with a timeout mechanism
        let timeout_seconds = 10;
        let mut attempts = 0;
        let max_attempts = timeout_seconds * 10; // 100ms intervals

        loop {
            let lock_result = libc::pthread_mutex_trylock(mutex_ptr);

            if lock_result == 0 {
                // Successfully acquired the mutex
                println!("Child: Mutex acquired after {} attempts!", attempts);
                break;
            } else if lock_result == libc::EBUSY {
                // Mutex is busy, wait and try again
                attempts += 1;
                if attempts >= max_attempts {
                    eprintln!(
                        "Child: Timeout waiting for mutex after {} seconds",
                        timeout_seconds
                    );
                    return Err("Child: Timeout waiting for mutex".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            } else {
                eprintln!("Child: Failed to acquire mutex with error: {}", lock_result);
                return Err(format!("Child: Failed to acquire mutex: {}", lock_result).into());
            }
        }

        // Read the current number
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        let current_number = *number_ptr;
        println!("Child: Current number: {}", current_number);

        // Child does math: add 25 and multiply by 2
        let new_number = (current_number + 25) * 2;
        *number_ptr = new_number;

        // Ensure the write is visible to other processes
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

        println!("Child: Applied operation ((n + 25) * 2)");
        println!("Child: New number: {}", new_number);

        // Simulate some work
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Release the mutex
        let unlock_result = libc::pthread_mutex_unlock(mutex_ptr);
        if unlock_result != 0 {
            eprintln!("Child: Failed to unlock mutex: {}", unlock_result);
            return Err(format!("Child: Failed to unlock mutex: {}", unlock_result).into());
        }
        println!("Child: Mutex released");
    }

    // Cleanup
    unsafe {
        libc::munmap(shared_ptr, SHARED_MEMORY_SIZE);
        libc::close(shm_fd);
    }

    println!("=== Child process finished ===");

    Ok(())
}
