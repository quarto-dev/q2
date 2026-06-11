//! Node discovery semantics (plan Phase 2, bd-81cfshmw): the
//! QUARTO_NODE override, PATH lookup, version-floor enforcement, and
//! the actionability of the not-found error.
//!
//! Fake node binaries are shell scripts, so the behavioral tests are
//! unix-only; `parse_version` and the error-shape tests are
//! platform-independent. Windows CI exercises the real-PATH path via
//! `discovery_from_env_finds_real_node` when node is present.

use quarto_mcp_launcher::{Discovery, MIN_NODE_MAJOR, NodeError, find_node};
use std::path::PathBuf;

fn empty_discovery() -> Discovery {
    Discovery {
        quarto_node: None,
        path_var: None,
        well_known: Vec::new(),
    }
}

#[cfg(unix)]
fn fake_node(dir: &std::path::Path, version: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("node");
    std::fs::write(&path, format!("#!/bin/sh\necho \"v{version}\"\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn missing_node_error_is_actionable() {
    let err = find_node(&empty_discovery()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("QUARTO_NODE"), "must name the override: {msg}");
    assert!(
        msg.contains(&format!("Node {MIN_NODE_MAJOR}+")),
        "must name the floor: {msg}"
    );
    assert!(msg.contains("nodejs.org"), "must say how to install: {msg}");
}

#[cfg(unix)]
#[test]
fn quarto_node_override_wins() {
    let dir = tempfile::tempdir().unwrap();
    let node = fake_node(dir.path(), "24.2.0");
    let d = Discovery {
        quarto_node: Some(node.clone().into_os_string()),
        // PATH would offer another node; the override must win.
        path_var: Some(dir.path().as_os_str().to_os_string()),
        well_known: Vec::new(),
    };
    let found = find_node(&d).unwrap();
    assert_eq!(found.path, node);
    assert_eq!(found.version, (24, 2, 0));
}

#[cfg(unix)]
#[test]
fn quarto_node_below_floor_is_an_error_not_a_fallthrough() {
    let dir = tempfile::tempdir().unwrap();
    let old_node = fake_node(dir.path(), "20.11.1");
    // A perfectly good node sits on PATH, but the explicit override
    // must NOT silently fall through to it.
    let good_dir = tempfile::tempdir().unwrap();
    fake_node(good_dir.path(), "24.0.0");
    let d = Discovery {
        quarto_node: Some(old_node.into_os_string()),
        path_var: Some(good_dir.path().as_os_str().to_os_string()),
        well_known: Vec::new(),
    };
    let err = find_node(&d).unwrap_err();
    assert!(matches!(err, NodeError::OverrideTooOld { .. }), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("20.11.1"), "{msg}");
}

#[cfg(unix)]
#[test]
fn path_lookup_finds_node() {
    let dir = tempfile::tempdir().unwrap();
    let node = fake_node(dir.path(), "25.1.2");
    let d = Discovery {
        quarto_node: None,
        path_var: Some(dir.path().as_os_str().to_os_string()),
        well_known: Vec::new(),
    };
    let found = find_node(&d).unwrap();
    assert_eq!(found.path, node);
    assert_eq!(found.version, (25, 1, 2));
}

#[cfg(unix)]
#[test]
fn too_old_path_node_falls_through_to_well_known() {
    let old_dir = tempfile::tempdir().unwrap();
    fake_node(old_dir.path(), "18.19.0");
    let wk_dir = tempfile::tempdir().unwrap();
    let good = fake_node(wk_dir.path(), "24.0.0");
    let d = Discovery {
        quarto_node: None,
        path_var: Some(old_dir.path().as_os_str().to_os_string()),
        well_known: vec![good.clone()],
    };
    let found = find_node(&d).unwrap();
    assert_eq!(found.path, good);
}

#[cfg(unix)]
#[test]
fn all_too_old_error_names_what_was_found() {
    let dir = tempfile::tempdir().unwrap();
    fake_node(dir.path(), "18.19.0");
    let d = Discovery {
        quarto_node: None,
        path_var: Some(dir.path().as_os_str().to_os_string()),
        well_known: Vec::new(),
    };
    let err = find_node(&d).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("18.19.0"),
        "rejected candidates should be named: {msg}"
    );
}

#[test]
fn discovery_from_env_finds_real_node_when_present() {
    // Smoke check against the actual environment: dev machines and CI
    // have Node 24+ (the repo's own toolchain), so this exercises the
    // real probe path end-to-end. Skip quietly when absent.
    let d = Discovery::from_env();
    match find_node(&d) {
        Ok(found) => {
            assert!(found.version.0 >= MIN_NODE_MAJOR);
            assert!(found.path.is_file() || found.path.is_symlink());
        }
        Err(NodeError::NotFound { .. }) => {
            eprintln!("no node on this machine; skipping");
        }
        Err(other) => panic!("unexpected discovery error: {other}"),
    }
}
