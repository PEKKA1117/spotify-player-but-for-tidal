//! End-to-end checks of the xtask binary against the real workspace.

use std::process::Command;

/// AC3: `cargo xtask layering` exits 0 on the real workspace.
#[test]
fn ac3_real_workspace_is_clean() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("layering")
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        .output()
        .expect("run xtask");
    assert!(
        out.status.success(),
        "layering failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
