/*
 * extension/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Quarto extension discovery, parsing, and metadata contribution.
 */

//! Quarto extension support.
//!
//! Extensions are discovered from `_extensions/` directories in the project
//! hierarchy and parsed from `_extension.yml` files. They can contribute
//! format-specific metadata, filters, shortcodes, and other resources.
//!
//! Built-in extensions (e.g. `quarto/lipsum`) are embedded in the binary
//! and discovered before user extensions, but user extensions with the
//! same name take priority (last-match-wins in `find_extension`).

pub mod discover;
pub(crate) mod paths;
pub mod read;
pub mod types;

// Native-only: builds an extension's TS engine bundle by shelling out to
// `deno bundle` (mirrors `engine::ts_process`'s native gate — unavailable
// on wasm32-unknown-unknown).
#[cfg(not(target_arch = "wasm32"))]
pub mod build;

pub use discover::{discover_extensions, discover_project_extensions, find_extension};
pub use read::read_extension;
pub use types::{Contributes, Extension, ExtensionFilter, ExtensionId};

/// Locate the built-in extensions directory for the current platform.
///
/// - **Native**: the embedded `resources/extensions/` bundle, lazily
///   extracted to a temp directory on first access.
/// - **WASM**: the `/__quarto_resources__/extensions` VFS path, when the
///   host has populated it.
///
/// Returns `None` when built-ins are unavailable (extraction failed, or
/// the VFS path is absent); discovery then proceeds with user
/// extensions only.
pub fn builtin_extensions_path(
    _runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> Option<std::path::PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        BUILTIN_EXTENSIONS.path().ok().map(|p| p.to_path_buf())
    }

    #[cfg(target_arch = "wasm32")]
    {
        let vfs_path = std::path::PathBuf::from("/__quarto_resources__/extensions");
        if _runtime
            .path_exists(&vfs_path, Some(quarto_system_runtime::PathKind::Directory))
            .unwrap_or(false)
        {
            Some(vfs_path)
        } else {
            None
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod builtin {
    use include_dir::{Dir, include_dir};

    use crate::resources::ResourceBundle;

    /// Built-in extensions embedded at compile time from `resources/extensions/`.
    static BUILTIN_EXTENSIONS_DIR: Dir =
        include_dir!("$CARGO_MANIFEST_DIR/../../resources/extensions");

    /// Resource bundle for built-in extensions. Lazily extracted to a temp
    /// directory on first access via `.path()`.
    pub static BUILTIN_EXTENSIONS: ResourceBundle =
        ResourceBundle::new("builtin-extensions", &BUILTIN_EXTENSIONS_DIR);

    /// Per-subtree embedded payloads for vendored extension subtrees (see
    /// `cargo xtask pull-extension-subtree`). Each real subtree registers its
    /// own `include_dir!` + `ResourceBundle` here, scoped to that subtree's
    /// `_extensions/` payload (never the whole vendored repo — see the
    /// extension-subtree-infrastructure plan's D1). Empty until the first
    /// real subtree (e.g. julia) is vendored.
    pub static EXTENSION_SUBTREE_PAYLOADS: &[&ResourceBundle] = &[];
}

#[cfg(not(target_arch = "wasm32"))]
pub use builtin::BUILTIN_EXTENSIONS;

/// Locate the builtin roots contributed by vendored extension subtrees (see
/// `cargo xtask pull-extension-subtree`), in registration order.
///
/// - **Dev/test seam**: `QUARTO_EXTENSION_SUBTREES_DIR` overrides everything
///   below with a single directory (pointed at a fixture in tests). Checked
///   before the embedded bundle, native-only — never read on WASM.
/// - **Native**: each registered per-subtree [`ResourceBundle`] in
///   [`builtin::EXTENSION_SUBTREE_PAYLOADS`], lazily extracted. Empty in
///   production until a real subtree is registered.
/// - **WASM**: the `/__quarto_resources__/extension-subtrees` VFS path, when
///   the host has populated it (see `populate_extension_subtrees` in
///   `wasm-quarto-hub-client`).
pub fn builtin_extension_subtree_roots(
    _runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> Vec<std::path::PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Ok(dir) = std::env::var("QUARTO_EXTENSION_SUBTREES_DIR") {
            return vec![std::path::PathBuf::from(dir)];
        }
        builtin::EXTENSION_SUBTREE_PAYLOADS
            .iter()
            .filter_map(|bundle| bundle.path().ok().map(|p| p.to_path_buf()))
            .collect()
    }

    #[cfg(target_arch = "wasm32")]
    {
        let vfs_path = std::path::PathBuf::from(format!(
            "{}/extension-subtrees",
            quarto_sass::RESOURCE_PATH_PREFIX
        ));
        if _runtime
            .path_exists(&vfs_path, Some(quarto_system_runtime::PathKind::Directory))
            .unwrap_or(false)
        {
            vec![vfs_path]
        } else {
            Vec::new()
        }
    }
}

/// All builtin extension roots, in scan order: the regular built-in
/// extensions dir first, then any vendored extension-subtree payloads.
/// Correct on every target (native and WASM) — the single helper every
/// discovery call site should use instead of calling
/// [`builtin_extensions_path`] directly, so a target-specific built-in
/// source is never accidentally left out at a given call site.
pub fn all_builtin_extension_roots(
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = builtin_extensions_path(runtime) {
        roots.push(path);
    }
    roots.extend(builtin_extension_subtree_roots(runtime));
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_runtime() -> quarto_system_runtime::NativeRuntime {
        quarto_system_runtime::NativeRuntime::new()
    }

    #[test]
    fn builtin_extension_subtree_roots_honors_env_override() {
        let runtime = make_runtime();
        let tmp = tempfile::TempDir::new().unwrap();

        // Each nextest test runs in its own process, so mutating the
        // process environment here is safe (no cross-test race).
        unsafe {
            std::env::set_var("QUARTO_EXTENSION_SUBTREES_DIR", tmp.path());
        }
        let roots = builtin_extension_subtree_roots(&runtime);
        unsafe {
            std::env::remove_var("QUARTO_EXTENSION_SUBTREES_DIR");
        }

        assert_eq!(roots, vec![tmp.path().to_path_buf()]);
    }

    #[test]
    fn builtin_extension_subtree_roots_empty_when_no_subtrees_registered() {
        let runtime = make_runtime();
        assert!(
            std::env::var("QUARTO_EXTENSION_SUBTREES_DIR").is_err(),
            "test process should not have this env var set"
        );

        // No subtree has been registered in `EXTENSION_SUBTREE_PAYLOADS` yet
        // (production default) — discovery should proceed with user
        // extensions only, same as the pre-pluralization `None` behavior.
        let roots = builtin_extension_subtree_roots(&runtime);
        assert!(roots.is_empty());
    }
}
