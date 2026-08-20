//! Phase 2 (bd-ee2fqm95; plan
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`):
//! the guest's local-vs-tunnel mode decision, plus the
//! Rust↔npm generator equivalence guard.
//!
//! The manifest *generator* tests live with the implementation in
//! `crates/spa-manifest` (both `build.rs` and the npm build must agree
//! with it byte-for-byte). The mode-decision tests stay here: the
//! decision consumes the *embedded* manifests, which this crate owns.

use quarto_preview::{
    AssetMode, AssetsBlock, EmbeddedManifests, PreviewUi, TunnelReason, decide_asset_mode,
};

/// 64-char lowercase hex stand-ins (the decision compares strings, but
/// the wire shape is a sha256 hex digest — keep the fixtures honest).
const VIEWER_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EDITOR_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OTHER_HASH: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn guest() -> EmbeddedManifests {
    EmbeddedManifests {
        viewer: Some(VIEWER_HASH.to_string()),
        editor: Some(EDITOR_HASH.to_string()),
    }
}

fn host(viewer: Option<&str>, editor: Option<&str>) -> AssetsBlock {
    AssetsBlock {
        viewer: viewer.map(str::to_string),
        editor: editor.map(str::to_string),
    }
}

/// Plan Phase 2, "match → Local": the host's advertised hash for the
/// session UI equals the guest's embedded manifest hash.
#[test]
fn decide_mode_local_on_hash_match() {
    let assets = host(Some(VIEWER_HASH), None);
    assert_eq!(
        decide_asset_mode(Some(&assets), PreviewUi::Viewer, &guest(), false),
        AssetMode::Local,
        "viewer hash match => Local"
    );
    // The editor session compares the *editor* hash (design decision
    // 5: viewer unless `editorBoot` is present).
    let assets = host(Some(OTHER_HASH), Some(EDITOR_HASH));
    assert_eq!(
        decide_asset_mode(Some(&assets), PreviewUi::Editor, &guest(), false),
        AssetMode::Local,
        "editor hash match => Local, even when the viewer hash differs"
    );
}

#[test]
fn decide_mode_tunnel_on_hash_mismatch() {
    // Host serves different bytes than the guest carries: never serve
    // locally.
    let assets = host(Some(OTHER_HASH), None);
    assert_eq!(
        decide_asset_mode(Some(&assets), PreviewUi::Viewer, &guest(), false),
        AssetMode::Tunnel(TunnelReason::HashMismatch),
    );
    // An editor session must not fall back to a matching *viewer*
    // hash — the UIs are different bundles.
    let assets = host(Some(VIEWER_HASH), Some(OTHER_HASH));
    assert_eq!(
        decide_asset_mode(Some(&assets), PreviewUi::Editor, &guest(), false),
        AssetMode::Tunnel(TunnelReason::HashMismatch),
    );
}

#[test]
fn decide_mode_tunnel_on_missing_manifest() {
    // Host omits `assets` entirely (placeholder embed / older host).
    assert_eq!(
        decide_asset_mode(None, PreviewUi::Viewer, &guest(), false),
        AssetMode::Tunnel(TunnelReason::HostHashMissing),
    );
    // Host advertises the block but not this session's UI.
    let assets = host(None, Some(EDITOR_HASH));
    assert_eq!(
        decide_asset_mode(Some(&assets), PreviewUi::Viewer, &guest(), false),
        AssetMode::Tunnel(TunnelReason::HostHashMissing),
    );
    // The guest's own embed has no manifest (fresh clone).
    // Self-healing: the next real build restores local mode.
    let no_guest = EmbeddedManifests::default();
    let assets = host(Some(VIEWER_HASH), None);
    assert_eq!(
        decide_asset_mode(Some(&assets), PreviewUi::Viewer, &no_guest, false),
        AssetMode::Tunnel(TunnelReason::GuestManifestMissing),
    );
}

#[test]
fn decide_mode_tunnel_under_spa_dir_override() {
    // `SPA_DIR_OVERRIDE` active on the guest => Tunnel regardless of
    // hashes (dev sessions serve disk bytes the manifest doesn't
    // describe). The host side never advertises `assets` under an
    // override either (pinned in config_endpoint.rs).
    let assets = host(Some(VIEWER_HASH), None);
    assert_eq!(
        decide_asset_mode(Some(&assets), PreviewUi::Viewer, &guest(), true),
        AssetMode::Tunnel(TunnelReason::SpaDirOverride),
    );
}

// ─── Rust ↔ npm generator equivalence ────────────────────────────────────────
//
// The viewer manifest's single producer is the npm build
// (`scripts/manifest-dist.mjs`, wired into q2-preview-spa's `build`
// script); the editor manifest's is `build.rs` via the `spa-manifest`
// crate. The two generators must produce the same manifest for the
// same tree or viewer hashes would mean different things depending on
// which implementation a future producer used. Pin the agreement on
// the real dist: parse the npm-written manifest, regenerate with the
// Rust generator, and require full structural equality.

#[test]
fn rust_generator_matches_the_npm_written_viewer_manifest() {
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../q2-preview-spa/dist");
    let manifest_path = dist.join(spa_manifest::MANIFEST_FILENAME);
    if !manifest_path.is_file() {
        // Fresh clone / dist not built: nothing to compare. The
        // placeholder branch of config_endpoint.rs covers that tree
        // state; this guard runs wherever the dist is real.
        eprintln!(
            "skipping: {} not found (dist not built)",
            manifest_path.display()
        );
        return;
    }
    let on_disk = spa_manifest::parse(
        &std::fs::read_to_string(&manifest_path).expect("read npm-written viewer manifest"),
    )
    .expect("npm-written manifest parses");
    let regenerated =
        spa_manifest::generate(spa_manifest::list_dir(&dist).expect("list viewer dist"))
            .expect("regenerate viewer manifest");

    assert_eq!(on_disk.version, regenerated.version);
    assert_eq!(
        on_disk.entries, regenerated.entries,
        "npm and Rust generators disagree on the entry list for the same dist"
    );
    assert_eq!(
        on_disk.hash, regenerated.hash,
        "npm and Rust generators disagree on the top-level hash for the same dist"
    );
}
