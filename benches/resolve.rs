use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let status = Command::new("sh")
        .arg("benches/resolve.sh")
        .args(std::env::args_os().skip(1).filter(|arg| arg != "--bench"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("failed to run benchmark script");

    ExitCode::from(status.code().unwrap_or(1) as u8)
}
