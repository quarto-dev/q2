//! `q2 mcp` launcher: embeds the bundled TypeScript hub MCP server
//! (`ts-packages/quarto-hub-mcp/dist-bundle/`, built by
//! `cargo xtask build-hub-mcp-bundle`), extracts it to a per-user
//! cache, finds an ambient Node.js, and delegates execution.
//!
//! The TS server is the canonical MCP implementation (auth, sync, tool
//! surface); this crate is deliberately a *thin* launcher so all
//! protocol and security behavior stays single-sourced. Design:
//! claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md (bd-81cfshmw).
//!
//! Launcher contract:
//! - never writes to stdout (it belongs to the MCP protocol), except
//!   for the explicit, pre-protocol `--launcher-info` query;
//! - all arguments pass through to the TS server verbatim (its
//!   `--help` is the user-facing usage);
//! - `QUARTO_NODE` overrides node discovery; `QUARTO_MCP_CACHE_DIR`
//!   overrides the cache location (mainly for tests).

mod bundle;
mod cache;
mod defaults;
mod delegate;
mod node;

pub use bundle::{
    BundleContents, BundleFile, LazyBytes, content_hash, embedded_files, is_placeholder,
};
pub use cache::{
    DEFAULT_MAX_AGE, ExtractedBundle, LAST_USED_FILE, LOCK_FILE, default_cache_root,
    extract_and_lock, gc,
};
pub use defaults::{BundledDefault, Source, bundled_defaults, classify, injections, sources};
pub use node::{Discovery, MIN_NODE_MAJOR, NodeError, NodeInfo, find_node, parse_version};

use anyhow::{Result, bail};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// What `run` should do with the args, decided up front by
/// [`classify_args`] before any I/O. `q2 mcp` is a verbatim passthrough
/// to the embedded TS server; this enumerates the few launcher-level
/// flags we intercept before delegating.
#[derive(Debug, PartialEq, Eq)]
pub enum LauncherAction {
    /// `--launcher-info` (sole arg): print embed/cache/node diagnostics
    /// and exit.
    LauncherInfo,
    /// `--print-config` (sole arg): print a ready-to-paste `.mcp.json`
    /// entry and exit.
    PrintConfig,
    /// `--help`/`-h` (anywhere): print the launcher-options preamble,
    /// then delegate `--help` to the server so its own usage shows too.
    HelpPreambleThenDelegate,
    /// Everything else: pass through to the server verbatim.
    Delegate,
}

/// Decide how to handle `q2 mcp` args. Pure (no I/O) so the
/// interception rules are unit-testable.
///
/// Precedence: `--help`/`-h` anywhere wins (so `q2 mcp --server x
/// --help` still helps); otherwise the sole-arg launcher queries
/// (`--launcher-info`, `--print-config`) match only when they are the
/// single argument — combined with anything else they flow through to
/// the server (which rejects the unknown flag). They are always invoked
/// alone in practice.
pub fn classify_args(args: &[String]) -> LauncherAction {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return LauncherAction::HelpPreambleThenDelegate;
    }
    if args.len() == 1 {
        match args[0].as_str() {
            "--launcher-info" => return LauncherAction::LauncherInfo,
            "--print-config" => return LauncherAction::PrintConfig,
            _ => {}
        }
    }
    LauncherAction::Delegate
}

/// A ready-to-paste `.mcp.json` entry that drives this binary as the
/// MCP server. Pure JSON on stdout so it pipes
/// (`q2 mcp --print-config > .mcp.json`). For a non-canonical hub,
/// append `"--server", "wss://your-hub/ws"` to `args`.
pub fn config_snippet() -> String {
    // A fixed literal (not serialized) so the formatting is stable and
    // matches the repo's own .mcp.json exactly.
    "{\n  \"mcpServers\": {\n    \"quarto-hub\": {\n      \"command\": \"q2\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}\n"
        .to_string()
}

/// The launcher-options section printed ahead of the server's own
/// `--help`. These controls live in the launcher and never reach the TS
/// server, so they are invisible in its usage block — this preamble is
/// where they are discoverable.
pub fn help_preamble() -> String {
    "\
q2 mcp — launch the Quarto Hub MCP server (embedded; needs Node.js).

Launcher options (handled by q2 before the server starts):
  --launcher-info       Print embed/cache/node diagnostics and exit
  --print-config        Print a .mcp.json entry for this server and exit
  --help, -h            Show this help (launcher options, then server options)

Launcher environment variables:
  QUARTO_NODE           Path to the Node.js binary (when PATH discovery fails)
  QUARTO_MCP_CACHE_DIR  Override the extracted-bundle cache location

Embedded MCP server options:
"
    .to_string()
}

/// Entry point for `q2 mcp [args…]`. Returns the exit code to use
/// (Windows delegation path); on Unix a successful delegation never
/// returns.
pub fn run(args: &[String]) -> Result<i32> {
    // stdout in the next two branches is correct: these are explicit,
    // pre-protocol queries, not a live MCP session (where stdout carries
    // JSON-RPC). The help preamble is printed before delegation below.
    match classify_args(args) {
        LauncherAction::LauncherInfo => {
            println!("{}", launcher_info()?);
            return Ok(0);
        }
        LauncherAction::PrintConfig => {
            print!("{}", config_snippet());
            return Ok(0);
        }
        LauncherAction::HelpPreambleThenDelegate => {
            // Launcher half first; the server's own `--help` follows via
            // delegation (args still carry --help/-h). On Unix the exec
            // below replaces this process, so this stdout flushes first.
            print!("{}", help_preamble());
        }
        LauncherAction::Delegate => {}
    }

    if bundle::is_placeholder() {
        bail!(
            "this q2 binary was built without the hub MCP bundle.\n\
             Run `cargo xtask build-hub-mcp-bundle`, then rebuild the q2 binary\n\
             (`cargo build --bin q2`). See AGENTS.md § hub MCP bundle."
        );
    }

    let files = extraction_files(bundle::embedded_files(), shared_wasm_lazy())?;
    let hash = bundle::content_hash(&files);
    let cache_root = cache_root()?;

    let extracted = cache::extract_and_lock(&cache_root, &files, &hash)?;
    // Opportunistic, best-effort; must never break the launch.
    cache::gc(&cache_root, &hash, cache::DEFAULT_MAX_AGE);

    let node = node::find_node(&node::Discovery::from_env())?;
    // Bundled quarto-hub.com defaults (release builds only): injected
    // into the child env for any hub variable the user hasn't set.
    let hub_defaults = defaults::bundled_defaults();
    let mut extra_env: Vec<(&str, &str)> =
        defaults::injections(&hub_defaults, |var| std::env::var(var).ok());
    // BP-10: the server reports this build identity on its MCP
    // Implementation record. The user env wins, as with the hub
    // defaults.
    let version_injection;
    if std::env::var_os("QUARTO_MCP_SERVER_VERSION").is_none() {
        version_injection = server_version_for_child(bundle::build_info_json());
        if let Some(v) = &version_injection {
            extra_env.push(("QUARTO_MCP_SERVER_VERSION", v.as_str()));
        }
    }
    // CAP-12: the render tools spawn `q2 render`; default to this binary.
    let q2_path_injection;
    if let Some(v) = q2_path_for_child(
        std::env::var_os("QUARTO_Q2_PATH"),
        std::env::current_exe().ok(),
    ) {
        q2_path_injection = v;
        extra_env.push(("QUARTO_Q2_PATH", q2_path_injection.as_str()));
    }
    delegate::delegate(
        &node.path,
        &extracted.dir.join("index.mjs"),
        args,
        &extra_env,
        extracted.lock,
    )
}

/// The shared render WASM as a lazy extraction-set entry, when this
/// binary embeds one. `quarto-wasm-embed`'s build.rs stamps the digest
/// and length from the very bytes it compresses, so stamp and payload
/// agree by construction (pinned by that crate's own tests).
fn shared_wasm_lazy() -> Option<LazyBytes> {
    Some(LazyBytes {
        sha256: quarto_wasm_embed::WASM_SHA256?,
        len: quarto_wasm_embed::WASM_LEN?,
        get: quarto_wasm_embed::wasm_bytes,
    })
}

/// The extraction set: the embedded bundle files, plus the shared
/// render WASM as a lazy entry under the fixed sibling name
/// `wasm-host.mjs` reads (`new URL('./wasm_quarto_hub_client_bg.wasm',
/// import.meta.url)`) when the embed carries no WASM of its own — the
/// dedupe case (`build.rs` strips it; plan
/// `claude-notes/plans/2026-10-13-wasm-embed-dedupe.md`). The lazy
/// entry keys the content hash by its build-time stamped digest, so a
/// warm-cache launch never decodes or hashes the ~36 MB payload; only
/// a cold extraction materializes it. A pre-dedupe bundle still embeds
/// its WASM and is extracted as-is. A stripped bundle with no shared
/// WASM embedded is a build-time inconsistency; the error names the
/// fix.
fn extraction_files(
    mut files: Vec<BundleFile>,
    shared_wasm: Option<LazyBytes>,
) -> Result<Vec<BundleFile>> {
    let has_wasm = files
        .iter()
        .any(|(path, _)| path == Path::new(quarto_wasm_embed::WASM_FILENAME));
    if has_wasm {
        return Ok(files);
    }
    let Some(lazy) = shared_wasm else {
        bail!(
            "this q2 binary embeds an MCP bundle without its render WASM, and no\n\
             shared WASM was embedded either. Run `cargo xtask build-q2-preview-spa`\n\
             (or rebuild the bundle with `cargo xtask build-hub-mcp-bundle`), then\n\
             rebuild the q2 binary (`cargo build --bin q2`)."
        );
    };
    if let Some(warning) =
        wasm_mismatch_warning(bundle::embedded_wasm_sha256().as_deref(), lazy.sha256)
    {
        // stderr only: stdout belongs to the MCP protocol, and the
        // launch itself must go on (most tools never render).
        eprintln!("{warning}");
    }
    files.push((
        PathBuf::from(quarto_wasm_embed::WASM_FILENAME),
        BundleContents::Lazy(lazy),
    ));
    Ok(files)
}

/// The stderr tripwire for a dev-time staleness state: the bundle's
/// glue was built against the WASM its `build-info.json` records; the
/// shared embed's build-time stamped sha256 names whatever WASM source
/// its build.rs found at q2-build time. A mismatch means the two
/// payloads came from different WASM generations. Never fatal — a
/// genuinely incompatible glue fails loudly at init on its own.
fn wasm_mismatch_warning(recorded_sha: Option<&str>, shared_sha256: &str) -> Option<String> {
    let recorded = recorded_sha?;
    if recorded == shared_sha256 {
        return None;
    }
    Some(format!(
        "q2 mcp: warning: the embedded MCP bundle was built against render WASM\n\
         {recorded}, but this binary embeds {shared_sha256};\n\
         get_errors may fail at init. Run `cargo xtask build-hub-mcp-bundle` and\n\
         `cargo xtask build-q2-preview-spa`, then rebuild q2."
    ))
}

/// The `QUARTO_MCP_SERVER_VERSION` value injected into the node child
/// (BP-10): the workspace version plus the embedded bundle's build
/// stamp — `0.32.0+5a41c8bea`, or with `.dirty` for a dirty-tree
/// bundle. None for placeholder builds (the server then falls back to
/// its own build-info stamp / package floor).
fn server_version_for_child(build_info: Option<&str>) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(build_info?).ok()?;
    let commit = parsed.get("gitCommit")?.as_str()?;
    let short: String = commit.chars().take(9).collect();
    let dirty = parsed
        .get("gitDirty")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some(format!(
        "{}+{}{}",
        env!("CARGO_PKG_VERSION"),
        short,
        if dirty { ".dirty" } else { "" }
    ))
}

/// The `QUARTO_Q2_PATH` value injected into the node child (CAP-12): the
/// MCP server's render tools spawn `q2 render` as a subprocess, and the
/// sane default is the very binary hosting `q2 mcp`. The user env wins,
/// as with the hub defaults and BP-10's version injection. `None` when
/// the current exe path is not UTF-8 (never on a sane install; the render
/// tools then fall back to `q2` on PATH).
fn q2_path_for_child(user_env: Option<OsString>, current_exe: Option<PathBuf>) -> Option<String> {
    if user_env.is_some() {
        return None;
    }
    current_exe?.to_str().map(str::to_owned)
}

fn cache_root() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("QUARTO_MCP_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    cache::default_cache_root()
}

/// Diagnostic blob for `q2 mcp --launcher-info`: which bundle is
/// embedded (the stale-embed tripwire — see AGENTS.md), where it
/// extracts, and which node would run it.
fn launcher_info() -> Result<String> {
    let mut out = String::new();
    // Hub-connection variable sources (env / bundled / absent). Values
    // are never printed: the source is the diagnostic, and the release
    // workflow asserts `: bundled` on all three. Reported even for
    // placeholder builds — the defaults are a property of the binary,
    // not of the embedded bundle.
    let hub_defaults = defaults::bundled_defaults();
    for (var, source) in defaults::sources(&hub_defaults, |v| std::env::var(v).ok()) {
        out.push_str(&format!("default {var}: {source}\n"));
    }
    if bundle::is_placeholder() {
        out.push_str("bundle: PLACEHOLDER (run `cargo xtask build-hub-mcp-bundle` and rebuild)\n");
        return Ok(out);
    }
    let files = bundle::embedded_files();
    let embeds_own_wasm = files
        .iter()
        .any(|(path, _)| path == Path::new(quarto_wasm_embed::WASM_FILENAME));
    let shared = shared_wasm_lazy();
    match extraction_files(files, shared) {
        Ok(files) => {
            out.push_str(&format!("bundle-hash: {}\n", bundle::content_hash(&files)));
            out.push_str(&format!("bundle-files: {}\n", files.len()));
        }
        Err(e) => out.push_str(&format!("bundle-hash: UNAVAILABLE ({e})\n")),
    }
    if embeds_own_wasm {
        out.push_str("render-wasm: embedded-in-bundle (pre-dedupe bundle)\n");
    } else if let Some(lazy) = shared {
        out.push_str(&format!(
            "render-wasm: shared ({} bytes, sha256 {})\n",
            lazy.len,
            lazy.sha256.get(..12).unwrap_or(lazy.sha256)
        ));
    } else {
        out.push_str("render-wasm: MISSING (run `cargo xtask build-q2-preview-spa` and rebuild)\n");
    }
    if let Some(info) = bundle::build_info_json() {
        out.push_str("build-info: ");
        out.push_str(info.trim());
        out.push('\n');
    }
    out.push_str(&format!("cache-root: {}\n", cache_root()?.display()));
    match node::find_node(&node::Discovery::from_env()) {
        Ok(node) => out.push_str(&format!(
            "node: {} (v{}.{}.{})\n",
            node.path.display(),
            node.version.0,
            node.version.1,
            node.version.2
        )),
        Err(e) => out.push_str(&format!("node: NOT FOUND — {}\n", e)),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn file(path: &str, contents: &[u8]) -> BundleFile {
        (
            PathBuf::from(path),
            BundleContents::Bytes(Cow::Owned(contents.to_vec())),
        )
    }

    /// A shared-WASM stand-in whose provider panics: the extraction-set
    /// code must key on the stamped identity and never materialize the
    /// ~36 MB payload (that happens only when extraction writes files).
    fn lazy(sha256: &'static str) -> LazyBytes {
        fn unused() -> Option<&'static [u8]> {
            panic!("the extraction set must not materialize the shared WASM");
        }
        LazyBytes {
            sha256,
            len: 12,
            get: unused,
        }
    }

    /// A pre-dedupe bundle (its own WASM inside) extracts as-is, even
    /// when a shared WASM exists.
    #[test]
    fn extraction_keeps_bundle_embedded_wasm_as_is() {
        let files = vec![
            file("index.mjs", b"mjs"),
            file(quarto_wasm_embed::WASM_FILENAME, b"wasm"),
        ];
        let out =
            extraction_files(files.clone(), Some(lazy("cafe"))).expect("self-contained bundle");
        assert_eq!(out.len(), 2);
        assert_eq!(
            bundle::content_hash(&out),
            bundle::content_hash(&files),
            "no shared append, no hash change"
        );
    }

    /// A dedupe-stripped bundle gets the shared WASM appended as a lazy
    /// entry under the fixed sibling name, and the hash tracks the
    /// stamped WASM identity without touching the bytes.
    #[test]
    fn extraction_appends_shared_wasm_under_the_fixed_name() {
        let files = vec![file("index.mjs", b"mjs")];
        let out =
            extraction_files(files.clone(), Some(lazy("cafe01"))).expect("shared wasm present");
        assert_eq!(out.len(), 2);
        let (path, contents) = &out[1];
        assert_eq!(path, Path::new(quarto_wasm_embed::WASM_FILENAME));
        let BundleContents::Lazy(appended) = contents else {
            panic!("the shared WASM rides as a lazy entry");
        };
        assert_eq!(appended.sha256, "cafe01");
        assert_ne!(
            bundle::content_hash(&out),
            bundle::content_hash(&files),
            "the cache key must track the WASM identity"
        );
        let out_other = extraction_files(files.clone(), Some(lazy("deadbeef"))).unwrap();
        assert_ne!(
            bundle::content_hash(&out),
            bundle::content_hash(&out_other),
            "different shared WASM, different cache key"
        );
    }

    /// A stripped bundle with no shared WASM anywhere is a build-time
    /// inconsistency; the error must name the fix.
    #[test]
    fn extraction_without_any_wasm_names_the_rebuild() {
        let files = vec![file("index.mjs", b"mjs")];
        let err = extraction_files(files, None).expect_err("no WASM anywhere");
        let msg = err.to_string();
        assert!(
            msg.contains("build-q2-preview-spa"),
            "error must point at the rebuild path: {msg}"
        );
    }

    #[test]
    fn server_version_joins_workspace_version_and_short_commit() {
        let info = r#"{"gitCommit": "5a41c8bea40dbf1a96a9fa050dd4ca2bd247a51", "gitDirty": false, "builtAt": "2026-10-06T10:20:32.545Z"}"#;
        assert_eq!(
            server_version_for_child(Some(info)).unwrap(),
            format!("{}+5a41c8bea", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn mismatch_warning_fires_only_on_real_mismatch() {
        assert!(wasm_mismatch_warning(None, "aaaa").is_none());
        assert!(wasm_mismatch_warning(Some("aaaa"), "aaaa").is_none());
        let warning = wasm_mismatch_warning(Some("deadbeef"), "cafe00").expect("mismatch warns");
        assert!(warning.contains("deadbeef"));
        assert!(warning.contains("cafe00"));
        assert!(warning.contains("build-hub-mcp-bundle"));
    }

    #[test]
    fn server_version_marks_a_dirty_tree() {
        let info = r#"{"gitCommit": "0123456789abcdef", "gitDirty": true}"#;
        assert_eq!(
            server_version_for_child(Some(info)).unwrap(),
            format!("{}+012345678.dirty", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn server_version_is_none_without_a_build_stamp() {
        assert_eq!(server_version_for_child(None), None);
        assert_eq!(server_version_for_child(Some("not json")), None);
        assert_eq!(
            server_version_for_child(Some(r#"{"gitDirty": false}"#)),
            None
        );
    }

    #[test]
    fn q2_path_injection_defaults_to_current_exe() {
        assert_eq!(
            q2_path_for_child(None, Some(PathBuf::from("/usr/local/bin/q2"))),
            Some("/usr/local/bin/q2".to_string())
        );
    }

    #[test]
    fn q2_path_injection_lets_the_user_env_win() {
        assert_eq!(
            q2_path_for_child(
                Some(std::ffi::OsString::from("/opt/custom/q2")),
                Some(PathBuf::from("/usr/local/bin/q2"))
            ),
            None
        );
    }

    /// Building a non-UTF-8 `OsString` is platform-specific, so the
    /// fixture is a `cfg` pair and the test itself stays portable
    /// (`.claude/rules/cross-platform.md`; bd-j0pgiips).
    #[cfg(unix)]
    fn non_utf8_os_string() -> std::ffi::OsString {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![0xff, 0xfe])
    }

    /// A lone surrogate is ill-formed UTF-16, so `to_str()` returns
    /// `None` for it just as it does for non-UTF-8 bytes on unix.
    #[cfg(windows)]
    fn non_utf8_os_string() -> std::ffi::OsString {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xD800])
    }

    #[test]
    fn q2_path_injection_skips_non_utf8_exe_paths() {
        let weird = non_utf8_os_string();
        assert_eq!(q2_path_for_child(None, Some(PathBuf::from(weird))), None);
    }
}
