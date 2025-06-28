use std::ffi::CString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::ptr;
use tempfile::NamedTempFile;

// Use a simpler approach with fixed offsets
const MUTEX_OFFSET: usize = 0;
const NUMBER_OFFSET: usize = 256; // Put number at a fixed offset
const SHARED_MEMORY_SIZE: usize = 4096;

unsafe fn get_mutex_ptr(shared_ptr: *mut u8) -> *mut libc::pthread_mutex_t {
    unsafe { shared_ptr.add(MUTEX_OFFSET) as *mut libc::pthread_mutex_t }
}

unsafe fn get_number_ptr(shared_ptr: *mut u8) -> *mut i64 {
    unsafe { shared_ptr.add(NUMBER_OFFSET) as *mut i64 }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 64-bit Parent Process Started ===");
    println!("Process ID: {}", std::process::id());

    // Create shared memory name
    let shm_name = CString::new("/shared_memory_demo")?;

    // Clean up any existing shared memory
    unsafe {
        libc::shm_unlink(shm_name.as_ptr());
    }

    // Create shared memory
    let shm_fd = unsafe { libc::shm_open(shm_name.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o666) };

    if shm_fd == -1 {
        return Err("Failed to create shared memory".into());
    }

    // Set the size of shared memory
    if unsafe { libc::ftruncate(shm_fd, SHARED_MEMORY_SIZE as i64) } == -1 {
        unsafe {
            libc::close(shm_fd);
        }
        return Err("Failed to set shared memory size".into());
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
        return Err("Failed to map shared memory".into());
    }

    let shared_base = shared_ptr as *mut u8;
    let mutex_ptr = unsafe { get_mutex_ptr(shared_base) };
    let number_ptr = unsafe { get_number_ptr(shared_base) };

    // Initialize the mutex for inter-process use
    unsafe {
        let mut mutex_attr: libc::pthread_mutexattr_t = std::mem::zeroed();
        let attr_init_result = libc::pthread_mutexattr_init(&mut mutex_attr);
        if attr_init_result != 0 {
            return Err(format!(
                "Failed to initialize mutex attributes: {}",
                attr_init_result
            )
            .into());
        }

        let pshared_result =
            libc::pthread_mutexattr_setpshared(&mut mutex_attr, libc::PTHREAD_PROCESS_SHARED);
        if pshared_result != 0 {
            return Err(format!("Failed to set mutex process-shared: {}", pshared_result).into());
        }

        let mutex_init_result = libc::pthread_mutex_init(mutex_ptr, &mutex_attr);
        if mutex_init_result != 0 {
            return Err(format!("Failed to initialize mutex: {}", mutex_init_result).into());
        }
        libc::pthread_mutexattr_destroy(&mut mutex_attr);

        // Initialize the shared number
        *number_ptr = 100;

        // Ensure the write is visible to other processes
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

        // Lock the mutex initially
        let lock_result = libc::pthread_mutex_lock(mutex_ptr);
        if lock_result != 0 {
            return Err(format!("Failed to lock mutex initially: {}", lock_result).into());
        }
    }

    println!("Shared memory created and initialized");
    println!("Initial number: {}", unsafe { *number_ptr });
    println!("Mutex locked by parent");

    // Extract and spawn the child process
    let child_binary = include_bytes!(concat!(env!("OUT_DIR"), "/child_process_embedded"));
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(child_binary)?;
    temp_file.flush()?;

    // Make executable
    let mut perms = temp_file.as_file().metadata()?.permissions();
    perms.set_mode(0o755);
    temp_file.as_file().set_permissions(perms)?;

    // Convert to persistent file to close the file handle
    let temp_path = temp_file.into_temp_path();

    println!("\n=== Spawning 32-bit child process ===");

    // Spawn child process with shared memory name as argument
    let mut child = Command::new(&temp_path)
        .arg("/shared_memory_demo")
        .spawn()?;

    println!("Child process spawned with PID: {}", child.id());

    // Give child process time to start and wait for mutex
    std::thread::sleep(std::time::Duration::from_millis(500));

    println!("\n=== Parent unlocking mutex ===");

    // Unlock the mutex so child can proceed
    unsafe {
        let unlock_result = libc::pthread_mutex_unlock(mutex_ptr);
        if unlock_result != 0 {
            return Err(format!("Parent: Failed to unlock mutex: {}", unlock_result).into());
        }
    }

    println!("Parent: Mutex unlocked, child should now acquire it");

    // Wait for child to complete first
    println!("\n=== Parent waiting for child to complete ===");
    let exit_status = child.wait()?;
    println!("Child process completed with status: {}", exit_status);

    if !exit_status.success() {
        return Err("Child process failed".into());
    }

    println!("\n=== Parent attempting final operations ===");

    // Now acquire the lock for final operations
    unsafe {
        println!("Parent: Acquiring mutex for final operations...");
        let lock_result = libc::pthread_mutex_lock(mutex_ptr);
        if lock_result != 0 {
            return Err(format!("Parent: Failed to acquire mutex: {}", lock_result).into());
        }
        println!("Parent: Mutex acquired!");

        // Read the number modified by child
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        let current_number = *number_ptr;
        println!("Parent: Number after child processing: {}", current_number);

        // Parent does its own math: multiply by 3 and add 50
        let new_number = current_number * 3 + 50;
        *number_ptr = new_number;

        // Ensure the write is complete
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

        println!("Parent: Applied operation (n * 3 + 50)");
        println!("Parent: Final result: {}", new_number);

        // Unlock the mutex
        libc::pthread_mutex_unlock(mutex_ptr);
    }
    println!(
        "\n=== Child process completed with status: {} ===",
        exit_status
    );

    // Cleanup
    unsafe {
        libc::pthread_mutex_destroy(mutex_ptr);
        libc::munmap(shared_ptr, SHARED_MEMORY_SIZE);
        libc::close(shm_fd);
        libc::shm_unlink(shm_name.as_ptr());
    }

    // Clean up the temporary executable file
    let _ = fs::remove_file(&temp_path);

    println!("=== Parent process finished ===");

    Ok(())
}
