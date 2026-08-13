//! Phase 2 skeletons (bd-ee2fqm95; plan
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`):
//! SPA asset manifests and the guest's local-vs-tunnel mode decision.
//!
//! These are **structural stubs**: the manifest generator and the mode
//! decision function do not exist yet, so each test is ignored with a
//! `todo!()` body and a precise spec. Phase 2 starts by filling in the
//! bodies against the new API (compile-red, the accepted structural
//! failure mode), then implements.
//!
//! Placement note: the plan has `cargo xtask build-q2-preview-spa`
//! write the viewer manifest and `build.rs` write the editor manifest
//! (only `build.rs` knows the post-dedupe file set). The shared
//! generation logic therefore wants a home both can call — likely a
//! small shared crate (xtask dep + quarto-preview build-dep). If so,
//! move the generator tests there; the mode-decision tests stay here
//! (the decision consumes the *embedded* manifest, which this crate
//! owns).

/// Design decision 4: manifest generation is deterministic — sorted
/// `(path, sha256, size, content_type, content_encoding?)` entries plus
/// a top-level hash — so regenerating over an unchanged tree is
/// byte-identical (release CI builds on different platforms/jobs must
/// produce equal hashes or cross-platform local mode silently
/// disables).
#[test]
#[ignore = "Phase 2 skeleton (bd-ee2fqm95): manifest generator does not exist yet"]
fn manifest_regeneration_is_byte_identical() {
    // Spec: build a small fixture tree on disk (a few files with known
    // bytes, incl. one with a `.br` sibling so `content_encoding` is
    // exercised), run the generator twice into separate outputs, assert
    // byte-identical. Also assert entries are sorted by path and that
    // the output does not list the manifest file itself.
    todo!("Phase 2: generate_manifest(dir) twice -> byte-identical, sorted, self-excluding")
}

/// The top-level hash is the compatibility signal: any asset byte
/// change must change it (a guest with a stale embed must mismatch and
/// fall back to tunneling).
#[test]
#[ignore = "Phase 2 skeleton (bd-ee2fqm95): manifest generator does not exist yet"]
fn manifest_hash_changes_when_any_asset_byte_changes() {
    // Spec: generate over the fixture tree; flip one byte in one asset;
    // regenerate; assert the top-level hash differs (and that the
    // untouched entries are unchanged).
    todo!("Phase 2: one flipped asset byte -> different top-level hash")
}

/// A manifest cannot contain its own hash: the generator writes
/// `spa-manifest.json` into the dist dir, and the manifest's entry list
/// must exclude that file even when regenerated over a dist that
/// already contains a previous manifest.
#[test]
#[ignore = "Phase 2 skeleton (bd-ee2fqm95): manifest generator does not exist yet"]
fn manifest_excludes_itself_on_regeneration() {
    // Spec: generate once; regenerate over the same dir (manifest now
    // present); assert no entry's path is `spa-manifest.json` and the
    // two runs' top-level hashes are equal.
    todo!("Phase 2: regeneration over a manifested dir excludes the manifest itself")
}

/// The editor manifest records the *post-resolution* view — what
/// `lookup_embedded(Editor, path)` actually returns: editor-embed files
/// plus the viewer-embed fallback for stripped duplicates. Otherwise
/// editor-mode guests spuriously mismatch (plan risk: editor/viewer
/// dedupe).
#[test]
#[ignore = "Phase 2 skeleton (bd-ee2fqm95): manifest generator does not exist yet"]
fn editor_manifest_covers_post_resolution_view() {
    // Spec: fixture a viewer dist and an editor dist with one
    // byte-identical shared file (the dedupe target) and one
    // editor-only file; build the editor embed (shared file stripped);
    // the editor manifest must list BOTH the editor-only file and the
    // shared file (served via viewer fallback), each with the bytes the
    // runtime lookup would return.
    todo!("Phase 2: editor manifest = editor embed + viewer fallback, post-dedupe")
}

/// Guest-side mode decision (plan Phase 2, "match → Local"): the
/// host's advertised hash for the session UI equals the guest's
/// embedded manifest hash.
#[test]
#[ignore = "Phase 2 skeleton (bd-ee2fqm95): mode decision does not exist yet"]
fn decide_mode_local_on_hash_match() {
    // Spec: decide(host_assets: Option<&AssetsBlock>, ui, guest:
    // &EmbeddedManifests, override_active: bool) -> AssetMode; host
    // viewer hash == guest viewer hash => Local.
    todo!("Phase 2: hash match -> Local")
}

#[test]
#[ignore = "Phase 2 skeleton (bd-ee2fqm95): mode decision does not exist yet"]
fn decide_mode_tunnel_on_hash_mismatch() {
    // Spec: host viewer hash != guest viewer hash => Tunnel (bytes are
    // not the ones the host would serve; never serve locally).
    todo!("Phase 2: hash mismatch -> Tunnel")
}

#[test]
#[ignore = "Phase 2 skeleton (bd-ee2fqm95): mode decision does not exist yet"]
fn decide_mode_tunnel_on_missing_manifest() {
    // Spec: host omits `assets` (placeholder embed / older host) OR the
    // guest's own embed has no manifest (fresh clone) => Tunnel.
    // Self-healing: the next real build restores local mode.
    todo!("Phase 2: missing manifest on either side -> Tunnel")
}

#[test]
#[ignore = "Phase 2 skeleton (bd-ee2fqm95): mode decision does not exist yet"]
fn decide_mode_tunnel_under_spa_dir_override() {
    // Spec: `SPA_DIR_OVERRIDE` active on the guest => Tunnel regardless
    // of hashes (dev sessions serve disk bytes the manifest doesn't
    // describe). The host side never advertises `assets` under an
    // override either (pinned in config_endpoint.rs).
    todo!("Phase 2: override active -> Tunnel")
}
