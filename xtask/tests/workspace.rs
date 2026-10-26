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

/// Spec 0018 AC6 (Bug 2): a workspace crate depends on another workspace
/// crate with an exact requirement (`=x.y.z`), so a published crate needs
/// the crates it was released with, not the newest `1.x`.
#[test]
fn ac6_internal_dependencies_are_pinned() {
    use cargo_metadata::semver::Op;

    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        .no_deps()
        .exec()
        .expect("cargo metadata");
    let members = metadata.workspace_packages();
    let mut checked = 0;
    let mut loose = Vec::new();
    for package in &members {
        for dep in &package.dependencies {
            if dep.path.is_none() || !members.iter().any(|m| m.name.as_str() == dep.name) {
                continue;
            }
            checked += 1;
            let exact = matches!(dep.req.comparators.as_slice(), [c] if c.op == Op::Exact);
            if !exact {
                loose.push(format!("{} -> {} {}", package.name, dep.name, dep.req));
            }
        }
    }
    assert!(checked > 0, "no internal dependencies found");
    assert!(
        loose.is_empty(),
        "internal dependencies must use `version = \"=x.y.z\"`:\n{}",
        loose.join("\n")
    );
}
