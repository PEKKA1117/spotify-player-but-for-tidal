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

fn read_workflow(name: &str) -> String {
    let path = format!("{}/../.github/workflows/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// The workflow's steps, each starting with its name.
fn steps(yml: &str) -> Vec<&str> {
    yml.split("\n      - name: ").skip(1).collect()
}

/// AC7 (Bug 3): the release waits for the CI run of the push to `master` on
/// the tagged commit, not for a CI run of the tag, and only then publishes.
#[test]
fn ac7_release_waits_for_master_ci() {
    let yml = read_workflow("release.yml");
    let steps = steps(&yml);
    let wait = steps
        .iter()
        .position(|s| s.contains("gh run list"))
        .expect("a step that lists CI runs");
    let publish = steps
        .iter()
        .position(|s| s.starts_with("Publish to crates.io"))
        .expect("a 'Publish to crates.io' step");
    assert!(wait < publish, "the CI wait must come before publishing");
    let step = steps[wait];
    for flag in [
        "--workflow ci.yml",
        "--event push",
        "--branch master",
        "--commit \"${GITHUB_SHA}\"",
    ] {
        assert!(step.contains(flag), "wait step must pass `{flag}`:\n{step}");
    }
    assert!(
        !step.contains("--branch \"${GITHUB_REF_NAME}\""),
        "wait step must not look for the tag's own CI run:\n{step}"
    );
    assert!(
        step.contains("exit 1"),
        "wait step must fail when there is no run:\n{step}"
    );
    assert!(
        step.contains("gh run watch") && step.contains("--exit-status"),
        "wait step must watch the run with --exit-status:\n{step}"
    );
}

/// AC8 (Bug 3, spec 0001 AC9): CI runs on pushes to `master` and on pull
/// requests only, and its jobs on pull requests from `feat/*` and `bugfix/*`.
#[test]
fn ac8_ci_runs_once_per_commit() {
    let yml = read_workflow("ci.yml");
    let on: Vec<&str> = yml
        .split("\non:\n")
        .nth(1)
        .expect("an `on:` block")
        .lines()
        .take_while(|l| l.is_empty() || l.starts_with(' ') || l.starts_with('#'))
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    assert_eq!(
        on,
        ["  push:", "    branches: [master]", "  pull_request:"],
        "ci.yml's triggers"
    );

    let jobs = yml
        .lines()
        .filter(|l| l.trim() == "runs-on: ubuntu-latest")
        .count();
    let ifs: Vec<&str> = yml
        .lines()
        .filter(|l| l.starts_with("    if: "))
        .map(str::trim)
        .collect();
    assert!(jobs > 0, "ci.yml has jobs");
    assert_eq!(ifs.len(), jobs, "every job has an `if`: {ifs:?}");
    let expected = "if: ${{ github.event_name == 'push' || startsWith(github.head_ref, 'feat/') || startsWith(github.head_ref, 'bugfix/') || startsWith(github.head_ref, 'refactor/') }}";
    for line in ifs {
        assert_eq!(line, expected, "a job's `if`");
    }
}

/// Spec 0026 AC4: CI checks the specs against the code.
#[test]
fn ac4_ci_checks_the_specs() {
    let yml = read_workflow("ci.yml");
    assert!(
        yml.lines().any(|l| l.trim() == "run: cargo xtask specs"),
        "ci.yml runs `cargo xtask specs`"
    );
}
