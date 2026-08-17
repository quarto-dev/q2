/*
 * glob/expand.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Turning patterns into files.
//!
//! [`crate::glob::matcher`] answers "does this path match?"; this
//! module answers "which paths are there?". It walks through
//! [`SystemRuntime`], never `std::fs`, which is what lets
//! `resources:` expansion run in hub-client against the automerge
//! VFS as well as on a real filesystem. (The `glob` crate's own
//! `glob()` walker is the half of that crate q2 does *not* use —
//! see the module docs in [`crate::glob`].)
//!
//! # Pruning
//!
//! A naive implementation walks the whole project per pattern. This
//! one starts each walk at the pattern's longest literal prefix, so
//! `data/**/*.csv` descends into `data/` only, and never pays for
//! `_site/` or `node_modules/`. That is the same pruning the `glob`
//! crate's walker does, and it is what keeps the swap from
//! regressing on large projects.
//!
//! # What this module does *not* decide
//!
//! Which paths a subsystem refuses to look at — hidden files,
//! `_`-prefixed directories, the output directory — is the caller's
//! policy, not glob semantics (see
//! `claude-notes/designs/glob-semantics.md`). `resources:`
//! deliberately publishes `.nojekyll`; `project.render` deliberately
//! skips it. Callers that need exclusions filter the result.

use std::path::{Path, PathBuf};

use quarto_system_runtime::SystemRuntime;

use super::matcher::{GlobCompileError, PatternSet};
use super::pattern::{GlobPattern, has_metacharacters};
use super::{GlobOptions, path_to_forward_slashes};

/// Expand positive patterns into the files they match, minus
/// anything a negative pattern excludes.
///
/// Returned paths are **absolute** (`project_root` joined with the
/// match), de-duplicated, and sorted, so expansion is deterministic
/// across platforms and runtimes. Directories are never returned —
/// only the files beneath them.
///
/// Two things can go wrong: a pattern fails to compile, or the walk
/// itself fails (an unreadable directory — *not* a missing one,
/// which simply yields no matches). A pattern that matches nothing
/// is not an error here; the caller decides whether to diagnose
/// that, since "no matches" is normal for a defensive exclusion and
/// suspicious for a declared input.
pub fn expand(
    globs: &[GlobPattern],
    project_root: &Path,
    runtime: &dyn SystemRuntime,
    options: &GlobOptions,
) -> Result<Vec<PathBuf>, GlobExpandError> {
    let all = PatternSet::compile(globs, options)?;
    let mut out: Vec<PathBuf> = Vec::new();

    for glob in globs.iter().filter(|g| !g.negated) {
        let single = PatternSet::compile(std::slice::from_ref(glob), options)?;
        // `Path::join("")` appends a trailing separator, which some
        // runtimes read as a different directory than the bare path
        // (an in-memory VFS keyed on exact strings certainly does).
        // Keep the root untouched when the pattern has no literal
        // prefix to descend into.
        let prefix = literal_prefix(&glob.pattern);
        let start = if prefix.is_empty() {
            project_root.to_path_buf()
        } else {
            project_root.join(prefix)
        };

        for candidate in walk_files(&start, runtime)? {
            let Ok(relative) = candidate.strip_prefix(project_root) else {
                continue;
            };
            let rel_str = path_to_forward_slashes(relative);
            if single.matches(&rel_str) && !all.excluded(&rel_str) {
                out.push(candidate);
            }
        }
    }

    out.sort();
    out.dedup();
    Ok(out)
}

/// The leading path segments of `pattern` that contain no
/// metacharacters — the deepest directory a match can live under.
///
/// `data/**/*.csv` → `data`; `*.csv` → `""`; `a/b/c.csv` → `a/b`
/// (the final segment is dropped because it names the file, not a
/// directory to descend into).
fn literal_prefix(pattern: &str) -> String {
    let segments: Vec<&str> = pattern.split('/').collect();
    let mut prefix: Vec<&str> = Vec::new();
    for segment in segments.iter().take(segments.len().saturating_sub(1)) {
        if has_metacharacters(segment) {
            break;
        }
        prefix.push(segment);
    }
    prefix.join("/")
}

/// Something went wrong turning patterns into files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobExpandError {
    /// A pattern could not be compiled.
    Compile(GlobCompileError),
    /// A directory could not be read. A *missing* directory is not
    /// an error (the pattern just matches nothing); this is the
    /// permission-denied / IO-failure case, which the caller should
    /// surface rather than silently under-expand.
    Walk { path: PathBuf, message: String },
}

impl std::fmt::Display for GlobExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compile(e) => write!(f, "{e}"),
            Self::Walk { path, message } => {
                write!(f, "could not read `{}`: {message}", path.display())
            }
        }
    }
}

impl From<GlobCompileError> for GlobExpandError {
    fn from(e: GlobCompileError) -> Self {
        Self::Compile(e)
    }
}

/// Every file at or beneath `start`, through the runtime.
///
/// A **missing** directory yields nothing rather than an error: a
/// pattern pointing at a directory that does not exist simply
/// matches nothing. Any other read failure is surfaced, so a
/// permission problem cannot masquerade as an empty match set. If
/// `start` is itself a file, it is the only candidate.
fn walk_files(start: &Path, runtime: &dyn SystemRuntime) -> Result<Vec<PathBuf>, GlobExpandError> {
    let mut out = Vec::new();
    if runtime.is_file(start).unwrap_or(false) {
        out.push(start.to_path_buf());
        return Ok(out);
    }
    walk_rec(start, runtime, &mut out)?;
    Ok(out)
}

fn walk_rec(
    dir: &Path,
    runtime: &dyn SystemRuntime,
    out: &mut Vec<PathBuf>,
) -> Result<(), GlobExpandError> {
    let entries = match runtime.dir_list(dir) {
        Ok(entries) => entries,
        Err(e) if is_not_found(&e) => return Ok(()),
        Err(e) => {
            return Err(GlobExpandError::Walk {
                path: dir.to_path_buf(),
                message: e.to_string(),
            });
        }
    };
    for entry in entries {
        if runtime.is_dir(&entry).unwrap_or(false) {
            walk_rec(&entry, runtime, out)?;
        } else {
            out.push(entry);
        }
    }
    Ok(())
}

/// True for "this path does not exist", which expansion treats as an
/// empty match set rather than a failure.
fn is_not_found(err: &quarto_system_runtime::RuntimeError) -> bool {
    match err {
        quarto_system_runtime::RuntimeError::Io(e) => e.kind() == std::io::ErrorKind::NotFound,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::{PathKind, RuntimeError, RuntimeResult};
    use std::collections::BTreeSet;

    /// An in-memory runtime: no filesystem at all.
    ///
    /// This is the point of routing expansion through
    /// [`SystemRuntime`] — the same code that walks a real project
    /// walks hub-client's automerge VFS. `WasmRuntime` itself is
    /// `#[cfg(target_arch = "wasm32")]`, so a native test cannot
    /// instantiate it; this mock proves the property that matters
    /// (expansion never touches `std::fs`), and the wasm32 build leg
    /// of `cargo xtask verify` proves it compiles for the browser.
    struct MemoryRuntime {
        files: BTreeSet<PathBuf>,
    }

    impl MemoryRuntime {
        fn new(paths: &[&str]) -> Self {
            Self {
                files: paths.iter().map(PathBuf::from).collect(),
            }
        }

        fn is_dir_path(&self, path: &Path) -> bool {
            let prefix = format!("{}/", path.to_string_lossy());
            self.files
                .iter()
                .any(|f| f.to_string_lossy().starts_with(&prefix))
        }
    }

    #[async_trait::async_trait]
    impl SystemRuntime for MemoryRuntime {
        // ── the four methods this module actually exercises ──────
        fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
            Ok(match kind {
                Some(PathKind::File) => self.files.contains(path),
                Some(PathKind::Directory) => self.is_dir_path(path),
                Some(PathKind::Symlink) => false,
                None => self.files.contains(path) || self.is_dir_path(path),
            })
        }

        fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
            let prefix = format!("{}/", path.to_string_lossy());
            let mut children: BTreeSet<PathBuf> = BTreeSet::new();
            for file in &self.files {
                let full = file.to_string_lossy().to_string();
                if let Some(rest) = full.strip_prefix(&prefix) {
                    let head = rest.split('/').next().unwrap_or_default();
                    children.insert(PathBuf::from(format!("{prefix}{head}")));
                }
            }
            Ok(children.into_iter().collect())
        }

        fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
            if self.files.contains(path) {
                Ok(Vec::new())
            } else {
                Err(not_found())
            }
        }

        fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
            Ok(path.to_path_buf())
        }

        // ── unreachable from this module; stubbed so the mock can
        //    stand in for a full runtime ────────────────────────────
        fn file_write(&self, _p: &Path, _c: &[u8]) -> RuntimeResult<()> {
            Err(unsupported())
        }
        fn path_metadata(&self, _p: &Path) -> RuntimeResult<quarto_system_runtime::PathMetadata> {
            Err(unsupported())
        }
        fn file_copy(&self, _s: &Path, _d: &Path) -> RuntimeResult<()> {
            Err(unsupported())
        }
        fn path_rename(&self, _o: &Path, _n: &Path) -> RuntimeResult<()> {
            Err(unsupported())
        }
        fn file_remove(&self, _p: &Path) -> RuntimeResult<()> {
            Err(unsupported())
        }
        fn dir_create(&self, _p: &Path, _r: bool) -> RuntimeResult<()> {
            Err(unsupported())
        }
        fn dir_remove(&self, _p: &Path, _r: bool) -> RuntimeResult<()> {
            Err(unsupported())
        }
        fn cwd(&self) -> RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/proj"))
        }
        fn temp_dir(&self, _t: &str) -> RuntimeResult<quarto_system_runtime::TempDir> {
            Err(unsupported())
        }
        fn exec_pipe(&self, _c: &str, _a: &[&str], _s: &[u8]) -> RuntimeResult<Vec<u8>> {
            Err(unsupported())
        }
        fn exec_command(
            &self,
            _c: &str,
            _a: &[&str],
            _s: Option<&[u8]>,
        ) -> RuntimeResult<quarto_system_runtime::CommandOutput> {
            Err(unsupported())
        }
        fn env_get(&self, _k: &str) -> RuntimeResult<Option<String>> {
            Ok(None)
        }
        fn env_all(&self) -> RuntimeResult<std::collections::HashMap<String, String>> {
            Ok(std::collections::HashMap::new())
        }
        fn os_name(&self) -> &'static str {
            "memory"
        }
        fn arch(&self) -> &'static str {
            "memory"
        }
        fn cpu_time(&self) -> RuntimeResult<u64> {
            Ok(0)
        }
        fn xdg_dir(
            &self,
            _k: quarto_system_runtime::XdgDirKind,
            _p: Option<&Path>,
        ) -> RuntimeResult<PathBuf> {
            Err(unsupported())
        }
        fn stdout_write(&self, _b: &[u8]) -> RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _b: &[u8]) -> RuntimeResult<()> {
            Ok(())
        }
        async fn fetch_url(&self, _url: &str) -> RuntimeResult<(Vec<u8>, String)> {
            Err(unsupported())
        }
    }

    fn not_found() -> RuntimeError {
        RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "memory runtime",
        ))
    }

    fn unsupported() -> RuntimeError {
        RuntimeError::NotSupported("memory runtime".to_string())
    }

    fn project() -> MemoryRuntime {
        MemoryRuntime::new(&[
            "/proj/index.qmd",
            "/proj/.nojekyll",
            "/proj/data/public.csv",
            "/proj/data/secret.csv",
            "/proj/data/fig-1.png",
            "/proj/data/nested/deep.csv",
            "/proj/_site/stale.csv",
            "/proj/img/logo.png",
        ])
    }

    fn expand_str(patterns: &[GlobPattern]) -> Vec<String> {
        expand(
            patterns,
            Path::new("/proj"),
            &project(),
            &GlobOptions::default(),
        )
        .expect("compiles")
        .iter()
        .map(|p| {
            p.strip_prefix("/proj")
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect()
    }

    #[test]
    fn expands_a_glob_over_the_runtime() {
        assert_eq!(
            expand_str(&[GlobPattern::positive("data/*.csv")]),
            vec!["data/public.csv", "data/secret.csv"]
        );
    }

    #[test]
    fn recursive_glob_crosses_directories() {
        assert_eq!(
            expand_str(&[GlobPattern::positive("data/**/*.csv")]),
            vec!["data/nested/deep.csv", "data/public.csv", "data/secret.csv"]
        );
    }

    #[test]
    fn negation_subtracts() {
        assert_eq!(
            expand_str(&[
                GlobPattern::positive("data/*.csv"),
                GlobPattern::negated("data/secret.csv"),
            ]),
            vec!["data/public.csv"]
        );
    }

    #[test]
    fn character_classes_expand() {
        assert_eq!(
            expand_str(&[GlobPattern::positive("data/fig-[0-9].png")]),
            vec!["data/fig-1.png"]
        );
    }

    #[test]
    fn bare_directory_expands_to_everything_beneath() {
        assert_eq!(
            expand_str(&[GlobPattern::positive("data")]),
            vec![
                "data/fig-1.png",
                "data/nested/deep.csv",
                "data/public.csv",
                "data/secret.csv",
            ]
        );
    }

    #[test]
    fn a_literal_file_expands_to_itself() {
        assert_eq!(
            expand_str(&[GlobPattern::positive("img/logo.png")]),
            vec!["img/logo.png"]
        );
    }

    /// Dotfiles are matched: excluding them is the caller's policy.
    #[test]
    fn dotfiles_are_matched() {
        assert_eq!(expand_str(&[GlobPattern::positive("*")]).len(), 2);
        assert!(expand_str(&[GlobPattern::positive("*")]).contains(&".nojekyll".to_string()));
    }

    #[test]
    fn results_are_sorted_and_deduplicated() {
        // Two patterns matching the same file yield it once.
        assert_eq!(
            expand_str(&[
                GlobPattern::positive("data/public.csv"),
                GlobPattern::positive("data/*.csv"),
            ]),
            vec!["data/public.csv", "data/secret.csv"]
        );
    }

    #[test]
    fn a_pattern_matching_nothing_is_not_an_error() {
        assert!(expand_str(&[GlobPattern::positive("nowhere/*.csv")]).is_empty());
    }

    // ── pruning ─────────────────────────────────────────────────

    #[test]
    fn literal_prefix_is_the_deepest_safe_start() {
        assert_eq!(literal_prefix("data/**/*.csv"), "data");
        assert_eq!(literal_prefix("data/nested/*.csv"), "data/nested");
        assert_eq!(literal_prefix("*.csv"), "");
        assert_eq!(literal_prefix("a/b/c.csv"), "a/b");
        assert_eq!(literal_prefix("**/*.csv"), "");
        assert_eq!(literal_prefix("data"), "");
    }

    /// The pruning must not change *what* matches — only how much
    /// tree we touch. A pattern anchored under `data/` must still
    /// find everything there, and must not find `_site/`.
    #[test]
    fn pruning_does_not_change_results() {
        let all = expand_str(&[GlobPattern::positive("**/*.csv")]);
        assert!(all.contains(&"_site/stale.csv".to_string()));
        let pruned = expand_str(&[GlobPattern::positive("data/**/*.csv")]);
        assert!(!pruned.iter().any(|p| p.starts_with("_site/")));
        assert_eq!(pruned.len(), 3);
    }
}
