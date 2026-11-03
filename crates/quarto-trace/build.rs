//! Build script that exposes `QUARTO_GIT_HASH` as a compile-time env var.
//!
//! The hash format is `<short-hash>` or `<short-hash>-dirty` if the working
//! tree has uncommitted changes. When `git` is not available or `.git` is
//! missing (e.g. tarball builds via `cargo package`), the hash is `unknown`.

use std::process::Command;

fn main() {
    // A build driver that runs build scripts without git (corgi's sandbox)
    // computes the value itself and passes it in; see the [env] probe in
    // scripts/corgi/corgi.base.toml (bd-corgi-build-8fb3mzki). Same format:
    // `<short-hash>[-dirty]`.
    println!("cargo:rerun-if-env-changed=QUARTO_GIT_HASH");
    if let Ok(value) = std::env::var("QUARTO_GIT_HASH")
        && !value.is_empty()
    {
        println!("cargo:rustc-env=QUARTO_GIT_HASH={value}");
        return;
    }
    let hash = git_short_hash().unwrap_or_else(|| "unknown".to_string());
    let dirty = git_is_dirty().unwrap_or(false);
    let value = if hash == "unknown" {
        hash
    } else if dirty {
        format!("{}-dirty", hash)
    } else {
        hash
    };
    println!("cargo:rustc-env=QUARTO_GIT_HASH={}", value);

    // Re-run when HEAD or the index changes so the hash stays fresh. Ask
    // git where they live: in a linked worktree `.git` is a file, so a
    // hard-coded `../../.git/HEAD` never exists, and cargo treats a missing
    // watched path as always stale, rebuilding this crate and everything
    // above it (quarto-core, …) on every build (bd-7xlph6ki). Without git
    // (tarball builds) there is nothing to watch and the hash is `unknown`.
    for name in ["HEAD", "index"] {
        if let Some(path) = git_path(name) {
            quarto_build_support::watch_file(&path);
        }
    }
}

/// The path of `name` inside the git directory (`git rev-parse
/// --git-path`), resolved against the crate root, where cargo runs build
/// scripts.
fn git_path(name: &str) -> Option<std::path::PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", name])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        return None;
    }
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").ok()?);
    Some(manifest_dir.join(s))
}

fn git_short_hash() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn git_is_dirty() -> Option<bool> {
    // `--no-optional-locks`: a plain `git status` opportunistically rewrites
    // `.git/index` when cached stat info is stale (after a checkout, merge,
    // or CI's mtime restore). That index is a watched input of this very
    // script, so the write made the *next* build rerun it and recompile
    // quarto-core and everything above it (bd-7xlph6ki).
    let output = Command::new("git")
        .args(["--no-optional-locks", "status", "--porcelain"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(!output.stdout.is_empty())
}
