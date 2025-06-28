use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=child_process");

    let out_dir = env::var("OUT_DIR").unwrap();
    let target_dir = Path::new(&out_dir).join("child_build");

    std::fs::create_dir_all(&target_dir).unwrap();

    // Build the 32-bit child process
    let output = Command::new("cargo")
        .args(&[
            "build",
            "--release",
            "--target", "i686-unknown-linux-gnu",
            "--manifest-path", "child_process/Cargo.toml",
            "--target-dir", target_dir.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to build child process. Make sure you have: rustup target add i686-unknown-linux-gnu");

    if !output.status.success() {
        panic!(
            "Failed to build child process: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Copy the built executable
    let source = target_dir
        .join("i686-unknown-linux-gnu")
        .join("release")
        .join("child_process");

    let dest = Path::new(&out_dir).join("child_process_embedded");
    std::fs::copy(&source, &dest).unwrap();

    println!("Child process built successfully");
}
