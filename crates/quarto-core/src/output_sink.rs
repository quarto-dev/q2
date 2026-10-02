/*
 * output_sink.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Validated sink for destructive filesystem operations during render.
 *
 * Plan: claude-notes/plans/2026-05-20-render-truncates-source-images.md
 * Issue: bd-cfl67 (P0 data-loss)
 */

//! Validated sink for destructive filesystem operations.
//!
//! Producers (artifact writers, resource copiers, etc.) do not call
//! [`SystemRuntime::file_write`] / [`SystemRuntime::file_copy`]
//! directly with arbitrary paths. They enqueue *intents* — "write
//! these bytes to this path", "copy this source file to this
//! destination" — into an [`OutputSink`]. The sink validates every
//! destination against a declared set of `allowed_roots` (e.g. the
//! project's `output_dir`, the engine's intermediate dir) before any
//! disk mutation happens.
//!
//! This narrows the surface where destructive bugs can originate to
//! a single audited module. The historical incident motivating this
//! design — bd-cfl67, where `q2 render` truncated user-authored
//! source images to 0 bytes — is caught by the sink even when an
//! upstream producer regresses.
//!
//! # Two layers of validation
//!
//! * **Enqueue-time (`OutputSink::write` / `OutputSink::copy`)**:
//!   the destination, lexically normalized (resolving `.` and `..`),
//!   must be a descendant of at least one `allowed_roots` entry.
//!   This catches the bug class where producers feed back source
//!   paths as destinations.
//! * **Flush-time (`OutputSink::flush`)**: after parent-dir
//!   creation, the destination is canonicalized (which resolves
//!   symlinks) and re-checked against the canonicalized allowed
//!   roots. This catches symlink-based escapes the lexical check
//!   can't see. `Copy` ops additionally require that `src` and
//!   `dest` canonicalize to different paths.

use std::path::{Component, Path, PathBuf};

use quarto_system_runtime::SystemRuntime;

use crate::error::QuartoError;

/// A pending destructive operation against the filesystem.
#[derive(Debug)]
pub enum OutputOp {
    /// Write `bytes` to `dest`, creating parent directories as needed.
    Write { dest: PathBuf, bytes: Vec<u8> },
    /// Copy `src` to `dest`, creating parent directories as needed.
    Copy { src: PathBuf, dest: PathBuf },
}

impl OutputOp {
    fn dest(&self) -> &Path {
        match self {
            OutputOp::Write { dest, .. } => dest,
            OutputOp::Copy { dest, .. } => dest,
        }
    }
}

/// Summary of a successful [`OutputSink::flush`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlushReport {
    /// Number of `Write` ops executed.
    pub writes: usize,
    /// Number of `Copy` ops executed.
    pub copies: usize,
    /// Number of `Copy` ops skipped because `src` and `dest`
    /// canonicalize to the same path (the file is already where it
    /// needs to be).
    ///
    /// This is *not* an error. A producer that emits a copy whose
    /// `src` already lives at `dest` (e.g. an image referenced from
    /// a single-doc render whose output dir is the input dir) is
    /// expressing a correct intent — "ensure these bytes exist at
    /// this path." When they already do, the work is done. The
    /// distinct-from-error error is [`OutputSinkError::DestOutsideAllowedRoots`],
    /// which means the producer asked for something the sink will
    /// never agree to.
    pub copies_skipped_same_path: usize,
}

/// Reasons an enqueue or flush attempt was refused.
#[derive(Debug)]
pub enum OutputSinkError {
    /// `dest` is not a descendant of any declared `allowed_roots`
    /// entry. Distinct from a flush I/O failure: this is a contract
    /// violation by the producer.
    DestOutsideAllowedRoots {
        dest: PathBuf,
        allowed_roots: Vec<PathBuf>,
    },
    /// `dest` is not absolute. Producers must resolve destinations
    /// against a declared root before enqueueing.
    DestNotAbsolute { dest: PathBuf },
    /// `runtime.dir_create(parent)` failed during flush.
    CreateParent {
        parent: PathBuf,
        source: std::io::Error,
    },
    /// `runtime.canonicalize(parent)` failed during flush.
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
    /// `runtime.file_write(dest, bytes)` failed during flush.
    Write {
        dest: PathBuf,
        source: std::io::Error,
    },
    /// `runtime.file_copy(src, dest)` failed during flush.
    Copy {
        src: PathBuf,
        dest: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for OutputSinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputSinkError::DestOutsideAllowedRoots {
                dest,
                allowed_roots,
            } => write!(
                f,
                "output destination {} is not under any allowed root ({})",
                dest.display(),
                allowed_roots
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            OutputSinkError::DestNotAbsolute { dest } => {
                write!(f, "output destination must be absolute: {}", dest.display())
            }
            OutputSinkError::CreateParent { parent, source } => {
                write!(
                    f,
                    "failed to create parent dir {}: {}",
                    parent.display(),
                    source
                )
            }
            OutputSinkError::Canonicalize { path, source } => {
                write!(f, "failed to canonicalize {}: {}", path.display(), source)
            }
            OutputSinkError::Write { dest, source } => {
                write!(f, "failed to write {}: {}", dest.display(), source)
            }
            OutputSinkError::Copy { src, dest, source } => write!(
                f,
                "failed to copy {} -> {}: {}",
                src.display(),
                dest.display(),
                source
            ),
        }
    }
}

impl std::error::Error for OutputSinkError {}

impl From<OutputSinkError> for QuartoError {
    fn from(value: OutputSinkError) -> Self {
        QuartoError::other(value.to_string())
    }
}

/// A validated sink for destructive filesystem operations.
///
/// See the module-level docs for the design rationale and contract.
#[derive(Debug)]
pub struct OutputSink {
    /// Lexically-normalized allowed roots. Every enqueued `dest`
    /// must be a descendant of at least one of these.
    allowed_roots: Vec<PathBuf>,
    ops: Vec<OutputOp>,
}

impl OutputSink {
    /// Construct a sink that allows destructive writes only under
    /// the given roots. Each root is lexically normalized; the sink
    /// does not perform I/O at construction time.
    pub fn new(allowed_roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let allowed_roots = allowed_roots
            .into_iter()
            .map(|p| lexical_clean(&p))
            .collect();
        Self {
            allowed_roots,
            ops: Vec::new(),
        }
    }

    /// Enqueue a write of `bytes` to `dest`.
    ///
    /// `dest` must be absolute and lexically resolve under at least
    /// one allowed root. Returns the enqueue-time validation error
    /// otherwise; no disk state is mutated.
    pub fn write(&mut self, dest: PathBuf, bytes: Vec<u8>) -> Result<(), OutputSinkError> {
        self.validate_enqueue(&dest)?;
        self.ops.push(OutputOp::Write { dest, bytes });
        Ok(())
    }

    /// Enqueue a copy from `src` to `dest`.
    ///
    /// Same destination contract as [`OutputSink::write`]. The
    /// `src == dest` check is deferred to flush, where both paths
    /// can be canonicalized.
    pub fn copy(&mut self, src: PathBuf, dest: PathBuf) -> Result<(), OutputSinkError> {
        self.validate_enqueue(&dest)?;
        self.ops.push(OutputOp::Copy { src, dest });
        Ok(())
    }

    /// Number of pending operations.
    pub fn pending(&self) -> usize {
        self.ops.len()
    }

    fn validate_enqueue(&self, dest: &Path) -> Result<(), OutputSinkError> {
        // `has_root()` rather than `is_absolute()`: WASM's
        // `wasm32-unknown-unknown` target has no `target_family`, so
        // std reports paths like `/.quarto/project-artifacts/...`
        // (which the hub-client VFS uses as keys) as not absolute.
        // `has_root()` is true for any path beginning with `/`, and
        // that's the invariant we actually want here — every
        // destination must be rooted somewhere the producer can
        // declare, not a bare relative path.
        if !dest.has_root() {
            return Err(OutputSinkError::DestNotAbsolute {
                dest: dest.to_path_buf(),
            });
        }
        let cleaned = lexical_clean(dest);
        if self
            .allowed_roots
            .iter()
            .any(|root| cleaned.starts_with(root))
        {
            return Ok(());
        }
        // Fallback for OS-level path aliases (e.g. macOS's
        // `/var` ↔ `/private/var`): the resolver may have produced
        // a `dest` in one form and the caller may have declared
        // the allowed root in the other (e.g. via
        // `ProjectContext::single_file`, which canonicalizes the
        // project dir). Compare the deepest-existing-ancestor
        // canonical forms before rejecting. Native-only: WASM has no
        // OS-level aliasing to resolve.
        #[cfg(not(target_arch = "wasm32"))]
        {
            use quarto_system_runtime::canonicalize_deepest_existing;
            let canonical_dest = canonicalize_deepest_existing(&cleaned);
            if self.allowed_roots.iter().any(|root| {
                let canonical_root = canonicalize_deepest_existing(root);
                canonical_dest.starts_with(&canonical_root)
            }) {
                return Ok(());
            }
        }
        Err(OutputSinkError::DestOutsideAllowedRoots {
            dest: cleaned,
            allowed_roots: self.allowed_roots.clone(),
        })
    }

    /// Execute all pending operations against `runtime`.
    ///
    /// Operations run in enqueue order. For each op:
    /// 1. Parent directory is created (`dir_create(.., recursive)`).
    /// 2. The destination's parent is canonicalized; the resulting
    ///    `<canonical-parent>/<filename>` is checked against the
    ///    canonicalized allowed roots — this catches symlink-based
    ///    escapes that the enqueue-time lexical check can't see.
    /// 3. For `Copy`, `src` is canonicalized; if it equals the
    ///    canonical destination the op is skipped (counted in
    ///    `copies_skipped_same_path`).
    /// 4. The op runs via the runtime.
    ///
    /// Any I/O or validation failure aborts the flush. The sink is
    /// consumed; subsequent ops are dropped.
    pub fn flush(self, runtime: &dyn SystemRuntime) -> Result<FlushReport, OutputSinkError> {
        let mut report = FlushReport::default();
        if self.ops.is_empty() {
            return Ok(report);
        }

        // Materialize each allowed root before canonicalizing so
        // both sides of the under-root check use the same canonical
        // form. (On macOS, `/var/.../tmpX` canonicalizes to
        // `/private/var/.../tmpX`; if we canonicalize one side but
        // not the other, the textual prefix check fails even though
        // the destination is lexically inside the root.) `dir_create`
        // is idempotent / recursive so already-existing roots are
        // a no-op.
        let canonical_roots: Vec<PathBuf> = self
            .allowed_roots
            .iter()
            .map(|r| {
                runtime
                    .dir_create(r, true)
                    .map_err(|e| OutputSinkError::CreateParent {
                        parent: r.clone(),
                        source: io_error_from(e),
                    })?;
                runtime
                    .canonicalize(r)
                    .map_err(|e| OutputSinkError::Canonicalize {
                        path: r.clone(),
                        source: io_error_from(e),
                    })
            })
            .collect::<Result<_, _>>()?;

        for op in self.ops {
            let dest = op.dest().to_path_buf();
            let parent = dest.parent().ok_or_else(|| OutputSinkError::Canonicalize {
                path: dest.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "destination has no parent",
                ),
            })?;

            runtime
                .dir_create(parent, true)
                .map_err(|e| OutputSinkError::CreateParent {
                    parent: parent.to_path_buf(),
                    source: io_error_from(e),
                })?;

            let parent_canon =
                runtime
                    .canonicalize(parent)
                    .map_err(|e| OutputSinkError::Canonicalize {
                        path: parent.to_path_buf(),
                        source: io_error_from(e),
                    })?;
            let filename = dest
                .file_name()
                .ok_or_else(|| OutputSinkError::Canonicalize {
                    path: dest.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "destination has no file name",
                    ),
                })?;
            let dest_canon = parent_canon.join(filename);

            if !canonical_roots
                .iter()
                .any(|root| dest_canon.starts_with(root))
            {
                return Err(OutputSinkError::DestOutsideAllowedRoots {
                    dest: dest_canon,
                    allowed_roots: canonical_roots,
                });
            }

            match op {
                OutputOp::Write { dest: _, bytes } => {
                    runtime.file_write(&dest_canon, &bytes).map_err(|e| {
                        OutputSinkError::Write {
                            dest: dest_canon.clone(),
                            source: io_error_from(e),
                        }
                    })?;
                    report.writes += 1;
                }
                OutputOp::Copy { src, dest: _ } => {
                    // Canonicalize the source if it exists. If it
                    // doesn't, the runtime copy itself will produce
                    // a sensible error.
                    if let Ok(src_canon) = runtime.canonicalize(&src)
                        && src_canon == dest_canon
                    {
                        report.copies_skipped_same_path += 1;
                        continue;
                    }
                    runtime
                        .file_copy(&src, &dest_canon)
                        .map_err(|e| OutputSinkError::Copy {
                            src: src.clone(),
                            dest: dest_canon.clone(),
                            source: io_error_from(e),
                        })?;
                    report.copies += 1;
                }
            }
        }
        Ok(report)
    }
}

fn io_error_from(e: quarto_system_runtime::RuntimeError) -> std::io::Error {
    match e {
        quarto_system_runtime::RuntimeError::Io(e) => e,
        other => std::io::Error::other(other.to_string()),
    }
}

/// Lexical (non-I/O) normalization: drop `.` components, fold
/// `..` against the preceding normal component. Does not resolve
/// symlinks.
fn lexical_clean(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.last(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn root_path() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\out")
        } else {
            PathBuf::from("/out")
        }
    }

    fn under_root(rest: &str) -> PathBuf {
        let mut p = root_path();
        p.push(rest);
        p
    }

    /// R3 (bd-cfl67): destinations outside the declared
    /// `allowed_roots` are rejected at enqueue time.
    #[test]
    fn write_rejects_dest_outside_allowed_roots() {
        let mut sink = OutputSink::new([root_path()]);
        let escape = if cfg!(windows) {
            PathBuf::from("C:\\etc\\passwd")
        } else {
            PathBuf::from("/etc/passwd")
        };
        let err = sink
            .write(escape.clone(), b"oops".to_vec())
            .expect_err("write outside allowed_roots must be refused");
        match err {
            OutputSinkError::DestOutsideAllowedRoots { dest, .. } => {
                assert_eq!(dest, escape);
            }
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(sink.pending(), 0, "no op should have been enqueued");
    }

    /// R3 (bd-cfl67): same for copy.
    #[test]
    fn copy_rejects_dest_outside_allowed_roots() {
        let mut sink = OutputSink::new([root_path()]);
        let escape = if cfg!(windows) {
            PathBuf::from("C:\\etc\\passwd")
        } else {
            PathBuf::from("/etc/passwd")
        };
        let src = under_root("source.png");
        let err = sink
            .copy(src, escape.clone())
            .expect_err("copy outside allowed_roots must be refused");
        assert!(matches!(
            err,
            OutputSinkError::DestOutsideAllowedRoots { .. }
        ));
        assert_eq!(sink.pending(), 0);
    }

    /// R3 (bd-cfl67): a `..`-laden destination that lexically
    /// escapes the allowed root is refused even though the literal
    /// prefix matches.
    #[test]
    fn write_rejects_dotdot_escape_at_enqueue() {
        let mut sink = OutputSink::new([root_path()]);
        let mut dest = root_path();
        dest.push("..");
        dest.push("etc");
        dest.push("passwd");
        let err = sink
            .write(dest, b"x".to_vec())
            .expect_err("`..` escape must be refused");
        assert!(matches!(
            err,
            OutputSinkError::DestOutsideAllowedRoots { .. }
        ));
    }

    /// Destinations under a declared allowed root are accepted.
    #[test]
    fn write_accepts_dest_under_allowed_root() {
        let mut sink = OutputSink::new([root_path()]);
        let dest = under_root("doc_files/img.png");
        sink.write(dest, b"png".to_vec()).expect("under-root write");
        assert_eq!(sink.pending(), 1);
    }

    /// Multiple allowed roots: dest under any one of them is accepted.
    #[test]
    fn write_accepts_dest_under_any_root() {
        let mut sink = OutputSink::new([
            root_path(),
            if cfg!(windows) {
                PathBuf::from("C:\\intermediate")
            } else {
                PathBuf::from("/intermediate")
            },
        ]);
        let dest = if cfg!(windows) {
            PathBuf::from("C:\\intermediate\\figs\\fig1.png")
        } else {
            PathBuf::from("/intermediate/figs/fig1.png")
        };
        sink.write(dest, b"png".to_vec())
            .expect("dest under second allowed root");
    }

    /// Non-absolute destinations are refused at enqueue.
    #[test]
    fn write_rejects_relative_dest() {
        let mut sink = OutputSink::new([root_path()]);
        let err = sink
            .write(PathBuf::from("doc_files/img.png"), b"x".to_vec())
            .expect_err("relative dest must be refused");
        assert!(matches!(err, OutputSinkError::DestNotAbsolute { .. }));
    }

    /// R4 (bd-cfl67): on flush, a `Copy` whose source and destination
    /// canonicalize to the same path is **skipped** rather than
    /// executing a file_copy that would in principle truncate the
    /// source on some platforms. This is the runtime guard against
    /// the original bug shape — if a producer enqueues `copy(X, X)`,
    /// the sink turns it into a no-op and accounts for it in the
    /// flush report. Distinct from `DestOutsideAllowedRoots`, which
    /// is a hard producer-contract violation.
    #[test]
    fn copy_skips_when_src_equals_dest_on_flush() {
        use quarto_system_runtime::NativeRuntime;
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let file = root.join("img.png");
        std::fs::write(&file, b"\x89PNG-real-content").unwrap();
        let before = std::fs::read(&file).unwrap();

        let mut sink = OutputSink::new([root.clone()]);
        sink.copy(file.clone(), file.clone())
            .expect("enqueue OK (under allowed root)");

        let report = sink.flush(&NativeRuntime::new()).expect("flush OK");
        assert_eq!(report.copies, 0);
        assert_eq!(report.copies_skipped_same_path, 1);

        // The source file is byte-identical — the sink did not run
        // `file_copy` on it.
        let after = std::fs::read(&file).unwrap();
        assert_eq!(after, before, "source must not be modified");
    }

    /// Flush actually performs the writes / copies against the
    /// runtime when ops are well-formed.
    #[test]
    fn flush_executes_well_formed_ops() {
        use quarto_system_runtime::NativeRuntime;
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();

        let src = root.join("source.png");
        std::fs::write(&src, b"src-bytes").unwrap();

        let mut sink = OutputSink::new([root.clone()]);
        sink.write(root.join("written.txt"), b"hello".to_vec())
            .unwrap();
        sink.copy(src.clone(), root.join("copied.png")).unwrap();

        let report = sink.flush(&NativeRuntime::new()).unwrap();
        assert_eq!(report.writes, 1);
        assert_eq!(report.copies, 1);

        assert_eq!(std::fs::read(root.join("written.txt")).unwrap(), b"hello");
        assert_eq!(
            std::fs::read(root.join("copied.png")).unwrap(),
            b"src-bytes"
        );
        assert_eq!(
            std::fs::read(&src).unwrap(),
            b"src-bytes",
            "source unchanged"
        );
    }

    /// Lexical-clean of the allowed root means a path with `..`
    /// that actually resolves inside the root is accepted (e.g.
    /// `/out/figs/../img.png`). This is enqueue-side; flush-side
    /// canonicalize gives the same answer.
    #[test]
    fn write_accepts_dotdot_that_stays_inside_root() {
        let mut sink = OutputSink::new([root_path()]);
        let mut dest = root_path();
        dest.push("figs");
        dest.push("..");
        dest.push("img.png");
        sink.write(dest, b"x".to_vec())
            .expect("`..` that stays under root must be accepted");
    }
}
