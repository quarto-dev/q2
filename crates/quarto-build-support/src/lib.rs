//! Helpers shared by the workspace's build scripts (bd-7xlph6ki).
//!
//! Several build scripts embed a generated tree (an SPA `dist/`, a JS
//! bundle, the staged docs) that may or may not exist, and ask cargo to
//! rerun them when it changes. Two cargo behaviors make the obvious
//! `cargo:rerun-if-changed=<tree>` go wrong:
//!
//! - a watched path that does not exist is stale on *every* invocation,
//!   so watching a missing tree makes the crate, and everything above
//!   it, recompile on every `cargo build`/`cargo run` (GH #870);
//! - a watched directory is scanned *recursively*: it is stale when any
//!   file anywhere under it is newer than the build script's last run.
//!   Watching a missing tree's nearest existing ancestor instead is just
//!   as bad when that ancestor is the workspace root, whose `target/`
//!   the build itself keeps writing to.
//!
//! Use [`watch_dir`] and [`watch_file`] instead of emitting
//! `rerun-if-changed` by hand.
//!
//! Build scripts that rewrite an output read by `include_bytes!`
//! should use [`write_if_changed`], so a rerun that produces identical
//! bytes keeps the output's mtime and its dependents stay fresh.
//!
//! Build scripts that compress an embedded archive should use
//! [`embed_zstd_level`].

use std::path::{Component, Path, PathBuf};

/// Watched in place of a path that cannot be watched: the calling
/// build script itself (relative paths resolve against the package
/// root), which always exists and only changes when the script does.
/// Emitting at least one `rerun-if-changed` also keeps cargo from
/// falling back to rerunning on any change in the package.
const FALLBACK: &str = "build.rs";

/// Emit a `cargo:rerun-if-changed` that fires when the directory `path`
/// appears, vanishes, or anything under it changes.
///
/// A missing `path` whose parent exists is created as an empty
/// directory and watched; the generated trees this is used for are all
/// gitignored, and git does not track empty directories anyway. Its
/// mtime is backdated so creating it does not itself make the next
/// build stale; populating it later bumps the mtime and reruns the
/// script. When `path` cannot be created (missing parent, read-only
/// source tree) the script watches [`FALLBACK`], and a tree appearing
/// there later needs a `touch build.rs` to be picked up.
pub fn watch_dir(path: &Path) {
    println!("cargo:rerun-if-changed={}", dir_watch(path).display());
}

/// Emit a `cargo:rerun-if-changed` for the file `path`, or for
/// [`FALLBACK`] when it is missing. A missing file is not created; to
/// notice one appearing, also [`watch_dir`] its parent.
pub fn watch_file(path: &Path) {
    println!("cargo:rerun-if-changed={}", file_watch(path).display());
}

/// The path [`watch_dir`] watches, creating `path` when needed.
pub fn dir_watch(path: &Path) -> PathBuf {
    let path = normalize(path);
    if path.is_dir() || create_backdated_dir(&path).is_ok() {
        path
    } else {
        PathBuf::from(FALLBACK)
    }
}

/// The path [`watch_file`] watches.
pub fn file_watch(path: &Path) -> PathBuf {
    let path = normalize(path);
    if path.is_file() {
        path
    } else {
        PathBuf::from(FALLBACK)
    }
}

/// `create_dir` (not `create_dir_all`: a missing parent means the tree's
/// whole project is absent, and conjuring it would be surprising), then
/// set the mtime to the epoch. `AlreadyExists` is success: a sibling
/// build script watching the same tree may have just created it.
fn create_backdated_dir(path: &Path) -> std::io::Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => filetime::set_file_mtime(path, filetime::FileTime::zero()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(e) => Err(e),
    }
}

/// Resolve `.` and `..` components lexically, without touching the
/// filesystem. Build scripts build paths like
/// `<manifest_dir>/../../hub-client/dist`; normalizing keeps the watched
/// path (and cargo's "stale: changed" log lines) readable.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push(component);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Write `contents` to `path` unless it already holds exactly those
/// bytes. Returns whether the file was written. Leaving an identical
/// file untouched keeps its mtime, so an `include_bytes!` of it does not
/// force its crate to recompile.
pub fn write_if_changed(path: &Path, contents: &[u8]) -> std::io::Result<bool> {
    if std::fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(false);
    }
    std::fs::write(path, contents)?;
    Ok(true)
}

/// The zstd level for an embedded archive: 19 in release-like profiles,
/// so shipped binaries stay small, and 3 otherwise. A level-19 rerun cost
/// about 35s per `cargo` selection after every SPA rebuild, and level 3
/// about 4s, for a dev `q2` 2.8% larger (bd-e51t7k15).
pub fn embed_zstd_level() -> i32 {
    zstd_level_for(std::env::var("PROFILE").ok().as_deref())
}

/// Cargo sets `PROFILE` to `release` for `--release` and for profiles that
/// inherit from it, and to `debug` for the rest, including `ci`.
fn zstd_level_for(profile: Option<&str>) -> i32 {
    if profile == Some("release") { 19 } else { 3 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mtime(path: &Path) -> filetime::FileTime {
        filetime::FileTime::from_last_modification_time(&std::fs::metadata(path).unwrap())
    }

    #[test]
    fn existing_dir_watches_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("dist");
        std::fs::create_dir(&dist).unwrap();
        std::fs::write(dist.join("index.html"), "x").unwrap();
        let before = mtime(&dist);
        assert_eq!(dir_watch(&dist), dist);
        assert_eq!(mtime(&dist), before);
    }

    #[test]
    fn missing_dir_is_created_backdated_and_watched() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("agents-docs-dist");
        assert_eq!(dir_watch(&dist), dist);
        assert!(dist.is_dir());
        assert_eq!(std::fs::read_dir(&dist).unwrap().count(), 0);
        assert_eq!(mtime(&dist), filetime::FileTime::zero());
    }

    #[test]
    fn missing_dir_never_falls_back_to_an_ancestor() {
        // The ancestor would be scanned recursively by cargo, `target/`
        // included, so it would be stale after every build (bd-7xlph6ki).
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path().join("hub-client").join("dist");
        assert_eq!(dir_watch(&dist), PathBuf::from(FALLBACK));
        assert!(!tmp.path().join("hub-client").exists());
    }

    #[test]
    fn parent_dir_components_are_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let crate_dir = tmp.path().join("crates").join("quarto-preview");
        std::fs::create_dir_all(&crate_dir).unwrap();
        let dist = crate_dir.join("..").join("..").join("dist");
        assert_eq!(dir_watch(&dist), tmp.path().join("dist"));
        assert!(tmp.path().join("dist").is_dir());
    }

    #[test]
    fn existing_file_watches_itself_and_missing_file_falls_back() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("index.mjs");
        assert_eq!(file_watch(&file), PathBuf::from(FALLBACK));
        assert!(!file.exists());
        std::fs::write(&file, "x").unwrap();
        assert_eq!(file_watch(&file), file);
    }

    #[test]
    fn only_release_profiles_use_the_high_zstd_level() {
        assert_eq!(zstd_level_for(Some("release")), 19);
        assert_eq!(zstd_level_for(Some("debug")), 3);
        assert_eq!(zstd_level_for(None), 3);
    }

    #[test]
    fn write_if_changed_leaves_identical_files_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("archive.tar.zst");
        assert!(write_if_changed(&file, b"one").unwrap());
        assert!(!write_if_changed(&file, b"one").unwrap());
        assert!(write_if_changed(&file, b"two").unwrap());
        assert_eq!(std::fs::read(&file).unwrap(), b"two");
    }
}
