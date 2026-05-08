//! Smoke test: runs the binary briefly with tiny caps. Marked #[ignore] so it
//! doesn't run on shared CI by default — opt in with `cargo test -- --ignored`.

use std::process::Command;

fn binary_path() -> std::path::PathBuf {
    let mut p = std::env::current_exe().expect("test binary path");
    // tests/smoke-XXXXXX -> drop the test binary name and the deps/ folder
    p.pop(); // remove smoke-XXX
    if p.ends_with("deps") {
        p.pop();
    }
    p.push("btop-beautifier");
    p
}

#[test]
#[ignore]
fn runs_briefly_and_exits_clean() {
    // Build first so the binary exists.
    let build = Command::new("cargo")
        .args(["build", "--bin", "btop-beautifier"])
        .status()
        .expect("cargo build");
    assert!(build.success(), "cargo build failed");

    let bin = binary_path();
    assert!(bin.exists(), "binary not found at {}", bin.display());

    let output = Command::new(&bin)
        .args([
            "--duration", "3s",
            "--cpu-peak", "20",
            "--mem-cap", "50M",
            "--net-cap", "5M",
            "--reroll", "1s",
            "--seed", "1",
        ])
        .output()
        .expect("run binary");

    assert!(
        output.status.success(),
        "binary exited non-zero: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Driving CPU"), "missing startup banner: {}", stdout);
    assert!(stdout.contains("stopped cleanly"), "missing exit message: {}", stdout);
}
