//! Phase 2 of the live-share payload plan (bd-ee2fqm95;
//! `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`):
//! the embedded SPA asset manifests and the `--join` guest's
//! local-vs-tunnel mode decision.
//!
//! Each embedded bundle that was built by a real dist carries a
//! `spa-manifest.json` at its root (viewer: written by the npm build;
//! editor: written by `build.rs` over the post-resolution view). The
//! host advertises the manifests' top-level hashes in
//! `GET /api/preview/config` as the `assets` block; the guest compares
//! the hash for its session UI against its own embedded manifest and
//! serves assets locally iff they match exactly. Any mismatch, a
//! missing manifest on either side (fresh-clone placeholder embeds
//! ship none), or an active `SPA_DIR_OVERRIDE` falls back to full
//! tunneling — self-healing, and the trust boundary never moves: a
//! hash match means the guest already carries the very bytes the host
//! would serve.

use crate::{EMBEDDED_EDITOR, EMBEDDED_SPA, PreviewUi};

/// The `assets` block of `GET /api/preview/config` (design decision
/// 5): the top-level manifest hash of each embedded bundle, keyed by
/// UI. Fields are omitted when the corresponding embed has no manifest
/// (placeholder), and the host omits the whole block under
/// `SPA_DIR_OVERRIDE` (disk-served bytes are not described by the
/// embedded manifest). Both `Serialize` (the host serves it) and
/// `Deserialize` (the `q2 preview --join` CLI parses it) — one
/// wire-shape definition, no drift.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AssetsBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
}

/// This binary's own embedded manifest hashes, per UI. `None` for a
/// placeholder embed (fresh clone) — the guest then tunnels, and the
/// next real build restores local mode.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmbeddedManifests {
    pub viewer: Option<String>,
    pub editor: Option<String>,
}

/// Read the top-level hashes of the embedded manifests. The editor
/// hash comes from the *editor embed's own* manifest — never through
/// `lookup_embedded`'s viewer fallback, which would describe the wrong
/// bundle.
pub fn embedded_manifests() -> EmbeddedManifests {
    EmbeddedManifests {
        viewer: embedded_manifest(PreviewUi::Viewer).map(|m| m.hash),
        editor: embedded_manifest(PreviewUi::Editor).map(|m| m.hash),
    }
}

/// This binary's embedded manifest for `ui`, parsed. `None` on a
/// placeholder embed (fresh clone) — the guest then tunnels, and the
/// next real build restores local mode. The editor manifest comes from
/// the *editor embed's own* manifest file — never through
/// `lookup_embedded`'s viewer fallback (it describes the
/// post-resolution editor view; the viewer's manifest would describe
/// the wrong bundle). Phase 3's join frontend routes on the entry set.
pub fn embedded_manifest(ui: PreviewUi) -> Option<spa_manifest::Manifest> {
    let dir = match ui {
        PreviewUi::Viewer => &EMBEDDED_SPA,
        PreviewUi::Editor => &EMBEDDED_EDITOR,
    };
    let file = dir.get_file(spa_manifest::MANIFEST_FILENAME)?;
    let text = std::str::from_utf8(file.contents()).ok()?;
    spa_manifest::parse(text).ok()
}

/// Where a `--join` guest's asset requests go (Phase 3 acts on this;
/// Phase 2 logs the decision).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetMode {
    /// Host and guest embedded manifests agree: the guest's own binary
    /// carries the exact bytes the host would serve.
    Local,
    /// Full tunnel, as before Phase 3. The reason is carried for the
    /// log line.
    Tunnel(TunnelReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelReason {
    /// Hashes differ: the bytes are not the ones the host would serve.
    HashMismatch,
    /// Host omitted `assets` or this session's UI field (placeholder
    /// embed, `SPA_DIR_OVERRIDE` on the host, or an older host).
    HostHashMissing,
    /// This binary's own embed has no manifest (fresh clone).
    GuestManifestMissing,
    /// `SPA_DIR_OVERRIDE` active on the guest: it serves disk bytes
    /// the manifest doesn't describe.
    SpaDirOverride,
}

impl AssetMode {
    /// The join-path log line (plan Phase 2 specifies the first two
    /// strings verbatim).
    pub fn log_line(&self) -> String {
        match self {
            AssetMode::Local => "using embedded UI assets (hash match)".to_string(),
            AssetMode::Tunnel(reason) => {
                let detail = match reason {
                    TunnelReason::HashMismatch => "hash mismatch",
                    TunnelReason::HostHashMissing => "host did not advertise a manifest hash",
                    TunnelReason::GuestManifestMissing => "this binary embeds no asset manifest",
                    TunnelReason::SpaDirOverride => "SPA_DIR_OVERRIDE active",
                };
                format!("tunneling assets ({detail})")
            }
        }
    }
}

/// The guest's mode decision (design decisions 3–5). `host` is the
/// `assets` block from the join preflight's `/api/preview/config`
/// fetch (`None` when the fetch failed or the block was absent); `ui`
/// is the session UI (viewer unless `editorBoot` was present);
/// `override_active` is the guest's own `SPA_DIR_OVERRIDE` state.
pub fn decide_asset_mode(
    host: Option<&AssetsBlock>,
    ui: PreviewUi,
    guest: &EmbeddedManifests,
    override_active: bool,
) -> AssetMode {
    if override_active {
        return AssetMode::Tunnel(TunnelReason::SpaDirOverride);
    }
    let host_hash = host.and_then(|a| a.for_ui(ui));
    let guest_hash = match ui {
        PreviewUi::Viewer => guest.viewer.as_deref(),
        PreviewUi::Editor => guest.editor.as_deref(),
    };
    match (host_hash, guest_hash) {
        (Some(host), Some(guest)) if host == guest => AssetMode::Local,
        (Some(_), Some(_)) => AssetMode::Tunnel(TunnelReason::HashMismatch),
        (None, _) => AssetMode::Tunnel(TunnelReason::HostHashMissing),
        (Some(_), None) => AssetMode::Tunnel(TunnelReason::GuestManifestMissing),
    }
}

impl AssetsBlock {
    /// The advertised hash for one UI.
    pub fn for_ui(&self, ui: PreviewUi) -> Option<&str> {
        match ui {
            PreviewUi::Viewer => self.viewer.as_deref(),
            PreviewUi::Editor => self.editor.as_deref(),
        }
    }
}
