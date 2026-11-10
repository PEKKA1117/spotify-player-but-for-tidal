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

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root")
        .to_path_buf()
}

/// The workspace's Rust sources (not `target/`, not the vendored skills).
fn rust_sources() -> Vec<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let root = workspace_root();
    let mut out = Vec::new();
    walk(&root.join("crates"), &mut out);
    walk(&root.join("xtask/src"), &mut out);
    walk(&root.join("xtask/tests"), &mut out);
    out.sort();
    out
}

/// Spec 0026 AC3: `cargo xtask specs` exits 0 on the real workspace.
#[test]
fn ac3_specs_agree_with_the_code() {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("specs")
        .current_dir(workspace_root())
        .output()
        .expect("run xtask");
    assert!(
        out.status.success(),
        "specs failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "specs ok\n");
}

/// Spec 0026 AC6: a `mod tests` over 200 lines has its own file
/// (`main.rs` excepted).
#[test]
fn ac6_long_test_modules_have_their_own_file() {
    let mut long = Vec::new();
    for path in rust_sources() {
        if path.file_name().is_some_and(|n| n == "main.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read source");
        let lines: Vec<&str> = text.lines().collect();
        let Some(start) = lines.iter().position(|l| *l == "mod tests {") else {
            continue;
        };
        let end = lines[start..]
            .iter()
            .position(|l| *l == "}")
            .map_or(lines.len(), |n| start + n);
        if end - start - 1 > 200 {
            long.push(format!("{}: {} lines", path.display(), end - start - 1));
        }
    }
    assert!(
        long.is_empty(),
        "move these test modules to <module>/tests.rs:\n{}",
        long.join("\n")
    );
}

/// Spec 0026 AC7: no module is mounted with a path attribute.
#[test]
fn ac7_no_path_mounted_modules() {
    let attribute = concat!("#[", "path");
    let mounted: Vec<String> = rust_sources()
        .into_iter()
        .filter(|p| {
            std::fs::read_to_string(p)
                .expect("read source")
                .contains(attribute)
        })
        .map(|p| p.display().to_string())
        .collect();
    assert!(mounted.is_empty(), "path-mounted modules: {mounted:?}");
}
