mod shared;

use shared::SharedData;
use shared_memory::ShmemConf;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tempfile::NamedTempFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== 64-bit Parent Process Started ===");
    println!("Process ID: {}", std::process::id());

    let shmem = ShmemConf::new()
        .size(std::mem::size_of::<SharedData>())
        .create()?;

    println!("Shared memory created with OS ID: {}", shmem.get_os_id());

    let shared_data_ptr = shmem.as_ptr() as *mut SharedData;

    unsafe {
        std::ptr::write(shared_data_ptr, SharedData::new());
    }

    let shared_data = unsafe { &*shared_data_ptr };

    println!("Shared memory initialized");
    println!("Initial number: {}", shared_data.get_number());

    match shared_data.lock_timeout(std::time::Duration::from_secs(5)) {
        Ok(_) => println!("Parent has acquired the initial lock"),
        Err(e) => return Err(format!("Parent failed to acquire initial lock: {:?}", e).into()),
    }

    let child_binary = include_bytes!(concat!(env!("OUT_DIR"), "/child_process_embedded"));
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(child_binary)?;
    temp_file.flush()?;

    let mut perms = temp_file.as_file().metadata()?.permissions();
    perms.set_mode(0o755);
    temp_file.as_file().set_permissions(perms)?;

    let temp_path = temp_file.into_temp_path();

    println!("\n=== Spawning 32-bit child process ===");

    let mut child = Command::new(&temp_path).arg(shmem.get_os_id()).spawn()?;

    println!("Child process spawned with PID: {}", child.id());

    std::thread::sleep(std::time::Duration::from_millis(500));

    println!("\n=== Parent releasing lock ===");

    shared_data.unlock();
    println!("Parent: Lock released, child should now acquire it");

    println!("\n=== Parent waiting for child to complete ===");
    let exit_status = child.wait()?;
    println!("Child process completed with status: {}", exit_status);

    if !exit_status.success() {
        return Err("Child process failed".into());
    }

    println!("\n=== Parent performing final operations ===");

    println!("Parent: Acquiring lock for final operations...");
    match shared_data.lock_timeout(std::time::Duration::from_secs(5)) {
        Ok(_) => println!("Parent: Lock acquired!"),
        Err(e) => return Err(format!("Parent: Failed to acquire final lock: {:?}", e).into()),
    }

    let current_number = shared_data.get_number();
    println!("Parent: Number after child processing: {}", current_number);

    let new_number = current_number * 3 + 50;
    shared_data.set_number(new_number);

    println!("Parent: Applied operation (n * 3 + 50)");
    println!("Parent: Final result: {}", new_number);

    shared_data.unlock();
    println!("Parent: Lock released");

    println!(
        "\n=== Parent process completed successfully ===\n\
         Summary:\n\
         - Initial value: 100\n\
         - Child operation: ((n + 25) * 2) = {}\n\
         - Parent operation: (n * 3 + 50) = {}\n\
         - Architecture demo: ✓ 64-bit parent, 32-bit child\n\
         - Synchronization: ✓ Futex-based locking\n\
         - Memory sharing: ✓ Zero-copy inter-process communication",
        (100 + 25) * 2,
        new_number
    );

    let _ = fs::remove_file(&temp_path);

    Ok(())
}
