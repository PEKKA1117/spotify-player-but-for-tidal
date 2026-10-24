//! Checks of `.github/workflows/release.yml` (spec 0018).

/// AC4: the publish step runs `cargo xtask publish` with the registry token,
/// and nothing runs `cargo publish --workspace`.
#[test]
fn ac4_workflow_uses_xtask_publish() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/release.yml"
    );
    let yml = std::fs::read_to_string(path).expect("read release.yml");
    let step = yml
        .split("\n      - name: ")
        .find(|s| s.starts_with("Publish to crates.io"))
        .expect("a 'Publish to crates.io' step");
    assert!(
        step.contains("run: cargo xtask publish"),
        "publish step must run `cargo xtask publish`:\n{step}"
    );
    assert!(
        step.contains("CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_DEPLOY_KEY }}"),
        "publish step must set CARGO_REGISTRY_TOKEN:\n{step}"
    );
    for line in yml.lines().filter(|l| !l.trim_start().starts_with('#')) {
        assert!(
            !(line.contains("cargo publish") && line.contains("--workspace")),
            "no step may run `cargo publish --workspace`: {line}"
        );
    }
}
