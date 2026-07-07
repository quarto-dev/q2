/*
 * engine/ts_process.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * TS engine-host subprocess transport and demux.
 *
 * Gate: `#[cfg(not(target_arch = "wasm32"))]` — this entire module uses
 * `std::thread` and `std::process`, neither of which exists on wasm32.
 * See `engine-host-concurrency.md` and plan1a-host for the design.
 */

//! TS engine-host subprocess transport and demux.
//!
//! Implements:
//! - [`EngineTransport`] / [`EngineReadHalf`] — the split transport seam.
//! - [`StdioWriteHalf`] / [`StdioReadHalf`] — v1 newline-framed JSON over
//!   the child's stdin/stdout.
//! - [`TsEngineHost`] — the multiplexed demux: one reader thread, one pending
//!   map, per-request `sync_channel(1)` slots.
//! - [`MockTransport`] (`#[cfg(test)]`) — id-keyed, blocking `recv()`, for
//!   demux-tier tests that exercise the real reader thread.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, SyncSender},
};
use std::thread::JoinHandle;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

use crate::engine::error::ExecutionError;
use crate::engine::ts_protocol::{
    EngineProjectContext, FromEngine, HostGlobalConfig, LaunchEngineResult, LoadEngineResult,
    Request, Response, ToEngine,
};
use crate::stage::cancellation::Cancellation;

// ============================================================================
// Embedded bundle (SCOPE C)
// ============================================================================

/// The engine-host-deno bundle, embedded at compile time.
///
/// Path is anchored at `CARGO_MANIFEST_DIR` (the `quarto-core` crate root,
/// an absolute path known to rustc) — resilient to `ts_process.rs` moving
/// within the crate, unlike a source-file-relative `include_str!("../../../../…")`.
static EMBEDDED_BUNDLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ts-packages/quarto-engine-host-deno/dist/engine-host-deno.js"
));

/// Compute the 16-hex-char content hash (first 8 bytes of SHA-256) of a
/// byte slice.  Used for the content-hash-keyed filename under `bundles/`.
fn bundle_content_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(16);
    for byte in &digest[..8] {
        hex.push_str(&format!("{:02x}", byte));
    }
    hex
}

/// Extract the embedded bundle to `<bundles_dir>/engine-host-deno-<sha8>.js`.
///
/// - Write-if-absent: if the hash-named file already exists, reuse it.
/// - Atomic write: write to a sibling temp file in the same directory, then
///   rename — avoids partial writes and the Windows in-use-file hazard.
/// - Parameterised over `bundles_dir` so the unit test can point at a
///   scratch directory without touching the real runtime dir.
fn extract_bundle_to(bundles_dir: &Path) -> std::io::Result<PathBuf> {
    let bytes = EMBEDDED_BUNDLE.as_bytes();
    let hash = bundle_content_hash(bytes);
    let filename = format!("engine-host-deno-{hash}.js");
    let dest = bundles_dir.join(&filename);

    if dest.exists() {
        return Ok(dest);
    }

    std::fs::create_dir_all(bundles_dir)?;

    // Write atomically via a temp file in the same directory (same filesystem →
    // rename is atomic on POSIX; on Windows this avoids overwriting an in-use
    // fixed-name file since each hash gets its own name).
    let tmp_path = bundles_dir.join(format!("{filename}.tmp"));
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(&tmp_path, &dest)?;

    Ok(dest)
}

/// Cache for the once-extracted bundle path. Successes only.
/// `None` = not yet extracted (or last attempt failed); `Some(path)` = extracted.
static EXTRACTED_BUNDLE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Return the cached path, or run `extract` (with the lock released) and cache
/// only a successful result. A failed extraction is NOT cached — the next call
/// retries. Race-safe: `extract` is idempotent write-if-absent and the final
/// insert is double-checked.
fn cached_extract(
    cache: &Mutex<Option<PathBuf>>,
    extract: impl FnOnce() -> Result<PathBuf, ExecutionError>,
) -> Result<PathBuf, ExecutionError> {
    // Fast path: already cached.
    {
        let guard = cache.lock().unwrap();
        if let Some(ref path) = *guard {
            return Ok(path.clone());
        }
    } // guard dropped — lock released for extract()

    let path = extract()?; // Err returns early, never cached

    let mut guard = cache.lock().unwrap();
    // Another thread may have inserted while we extracted; eager get_or_insert
    // (NOT get_or_insert_with — that would run I/O under the guard).
    Ok(guard.get_or_insert(path).clone())
}

/// Extract the embedded bundle once, caching the result.
///
/// Writes to `<quarto_runtime_dir>/bundles/engine-host-deno-<sha8>.js`.
/// Subsequent calls return a clone of the cached path without I/O.
/// A failed extraction is NOT cached — the next call retries.
fn extracted_bundle_path() -> Result<PathBuf, ExecutionError> {
    cached_extract(&EXTRACTED_BUNDLE_PATH, || {
        let runtime_dir = quarto_util::quarto_runtime_dir()
            .map_err(|e| ExecutionError::other(format!("could not locate runtime dir: {e}")))?;
        let bundles_dir = runtime_dir.join("bundles");
        extract_bundle_to(&bundles_dir).map_err(|e| {
            ExecutionError::other(format!("failed to extract engine-host bundle: {e}"))
        })
    })
}

/// Check whether `deno` is available on PATH.
///
/// Spawns `deno --version` and checks for a successful exit status.
/// Returns `false` on any error (not found, permission denied, etc.).
pub fn is_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ============================================================================
// Transport traits
// ============================================================================

/// Shared write half of the engine transport — held by `TsEngineHost` as
/// `Arc<dyn EngineTransport>`, callable by concurrent workers.
///
/// Internally serialized by a short-held write lock (microseconds — one framed
/// write); NEVER held across a wait.
pub trait EngineTransport: Send + Sync {
    /// Write one framed `Request` to the child. Internally write-locked.
    fn send(&self, frame: &Request) -> Result<(), TransportError>;

    /// Send `Shutdown` and flush; then close the write channel (drop stdin).
    /// Does NOT reap the child — the host owns reaping (see Design Note
    /// "Teardown & reaping" in plan1a-host).
    fn shutdown(&self) -> Result<(), TransportError>;
}

/// Owned read half — moved into the demux reader thread at `ensure_started`.
/// Owns the child's stdout (v1) directly.
pub trait EngineReadHalf: Send {
    /// Block for the next frame.
    ///
    /// - `Ok(Response)` — a well-formed frame.
    /// - `Err(RecvError::Eof)` — channel closed (process exit or crash).
    /// - `Err(RecvError::Malformed(line))` — a line that fails to parse as
    ///   `Response` (the v1 stdout-protocol footgun). `reader_loop`'s caller
    ///   does NOT treat a single one of these as fatal — see
    ///   `MAX_CONSECUTIVE_MALFORMED_LINES` and the `Malformed` arm in
    ///   `reader_loop` for the bounded log-and-skip policy.
    /// - `Err(RecvError::Io(e))` — OS-level I/O error on the pipe.
    fn recv(&mut self) -> Result<Response, RecvError>;
}

/// Errors returned by [`EngineReadHalf::recv`].
#[derive(Debug)]
pub enum RecvError {
    /// Pipe closed — process exited or was killed.
    Eof,
    /// A line on the protocol channel failed to parse as a `Response`.
    /// Payload is the raw (bad) line for diagnostics.
    Malformed(String),
    /// OS-level I/O error reading the pipe.
    Io(std::io::Error),
}

/// Errors returned by [`EngineTransport::send`] / [`EngineTransport::shutdown`].
#[derive(Debug, thiserror::Error)]
#[error("transport error: {0}")]
pub struct TransportError(String);

impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        TransportError(e.to_string())
    }
}

impl From<serde_json::Error> for TransportError {
    fn from(e: serde_json::Error) -> Self {
        TransportError(e.to_string())
    }
}

// ============================================================================
// StdioWriteHalf / StdioReadHalf
// ============================================================================

/// Shared write half (held by the host as `Arc<StdioWriteHalf>`).
///
/// The inner `Option` is **required** (spike-validated): `shutdown(&self)` must
/// *close* stdin to give the child stdin-EOF, and you can only drop the
/// `BufWriter` out through `&self` by `take()`-ing it.
pub struct StdioWriteHalf {
    write: Mutex<Option<BufWriter<ChildStdin>>>,
}

impl StdioWriteHalf {
    fn new(stdin: ChildStdin) -> Self {
        Self {
            write: Mutex::new(Some(BufWriter::new(stdin))),
        }
    }
}

impl EngineTransport for StdioWriteHalf {
    fn send(&self, frame: &Request) -> Result<(), TransportError> {
        let mut guard = self.write.lock().unwrap();
        if let Some(writer) = guard.as_mut() {
            let line = serde_json::to_string(frame)?;
            writer.write_all(line.as_bytes())?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            Ok(())
        } else {
            Err(TransportError("stdin already closed".to_string()))
        }
    }

    fn shutdown(&self) -> Result<(), TransportError> {
        // Send Shutdown frame first (best-effort).
        {
            let mut guard = self.write.lock().unwrap();
            if let Some(writer) = guard.as_mut() {
                let shutdown_frame = Request {
                    id: u64::MAX, // throwaway id — no slot registered for Shutdown
                    msg: ToEngine::Shutdown,
                };
                if let Ok(line) = serde_json::to_string(&shutdown_frame) {
                    let _ = writer.write_all(line.as_bytes());
                    let _ = writer.write_all(b"\n");
                    let _ = writer.flush();
                }
            }
        }
        // Close stdin by taking the BufWriter out of the Option and dropping it.
        let _ = self.write.lock().unwrap().take();
        Ok(())
    }
}

/// Owned read half (moved into the demux reader thread).
///
/// Owns stdout directly — taken via `child.stdout.take()` — plus a clone of
/// the shared child handle to reap on crash-EOF.
pub struct StdioReadHalf {
    read: BufReader<ChildStdout>,
    /// Shared with host for single-shot reap in Part 2 proc-tier tests.
    /// Not read by the demux logic — `reader_loop` receives its own field-clone
    /// of the child Arc — but this field is the load-bearing handle that keeps
    /// the Arc alive from the read half's perspective.
    #[allow(dead_code)]
    pub(crate) child: Arc<Mutex<Option<Child>>>,
}

impl EngineReadHalf for StdioReadHalf {
    fn recv(&mut self) -> Result<Response, RecvError> {
        let mut line = String::new();
        match self.read.read_line(&mut line) {
            Ok(0) => Err(RecvError::Eof),
            Ok(_) => {
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                if trimmed.is_empty() {
                    return Err(RecvError::Eof);
                }
                serde_json::from_str::<Response>(trimmed)
                    .map_err(|_| RecvError::Malformed(trimmed.to_string()))
            }
            Err(e) => Err(RecvError::Io(e)),
        }
    }
}

/// Spawn a child from a `Command` with all three stdio pipes set up. Stores
/// the `Child` in the provided `child_slot` Arc (which is shared with the read
/// half), then returns the wired halves.
///
/// **For tests** that drive an arbitrary child command (a cat/echo script),
/// build a `Command` and call this function directly.
pub fn spawn_into(
    mut cmd: Command,
    child_slot: Arc<Mutex<Option<Child>>>,
) -> Result<
    (
        Arc<StdioWriteHalf>,
        StdioReadHalf,
        std::process::ChildStderr,
    ),
    ExecutionError,
> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| ExecutionError::other(format!("failed to spawn engine host: {e}")))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| ExecutionError::other("child stdin not available"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ExecutionError::other("child stdout not available"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ExecutionError::other("child stderr not available"))?;

    // Store the child in the shared slot.
    *child_slot.lock().unwrap() = Some(child);

    let write = Arc::new(StdioWriteHalf::new(stdin));
    let read = StdioReadHalf {
        read: BufReader::new(stdout),
        child: Arc::clone(&child_slot),
    };

    Ok((write, read, stderr))
}

// ============================================================================
// TsEngineHost — the demux
// ============================================================================

/// Per-request pending slot.
struct PendingSlot {
    /// Engine name (for crash broadcast naming).
    engine: String,
    /// Response delivery channel — capacity 1 so reader never blocks.
    tx: SyncSender<Result<FromEngine, ExecutionError>>,
}

/// The demux: one shared Deno subprocess, multiplexed over many in-flight
/// requests.  All multiplexing lives here — the transport is a dumb framed
/// duplex.
///
/// # Thread-safety
///
/// All mutable state is individually `Arc`'d so the reader thread can capture
/// field-clones without holding an `Arc<Self>` (which would prevent `Drop`
/// from running — the spike-confirmed deadlock described in plan1a-host).
pub struct TsEngineHost {
    /// Shared write half — set by `ensure_started`.
    ///
    /// Resettable (`Mutex<Option<..>>`, NOT `OnceLock`): a `ProcessCrashed`
    /// observation (dead subprocess) calls [`Self::reset_after_crash`],
    /// which clears this so the NEXT `ensure_started()` performs a genuine
    /// fresh spawn instead of reusing a transport whose stdin pipe is
    /// broken. Timeout/Cancel do NOT clear this — the process stays alive,
    /// so the existing transport remains valid and is reused (see
    /// `TsEngine::execute`'s poison guard, `ts_engine.rs`).
    write: Mutex<Option<Arc<dyn EngineTransport>>>,
    /// Shared child handle for kill/wait. `Option` for single-shot reap.
    child: Arc<Mutex<Option<Child>>>,
    /// Set before any kill so the reader can tell expected exit from crash.
    shutting_down: Arc<AtomicBool>,
    /// In-flight requests keyed by `id`.
    pending: Arc<Mutex<HashMap<u64, PendingSlot>>>,
    /// Monotonically increasing request id.
    next_id: AtomicU64,
    /// The demux (stdout) reader thread.
    reader: Mutex<Option<JoinHandle<()>>>,
    /// The stderr reader/forwarder thread.
    stderr_reader: Mutex<Option<JoinHandle<()>>>,
    /// Bounded ring of recent stderr lines (cap ~100) for crash diagnostics.
    recent_stderr: Arc<Mutex<VecDeque<String>>>,
    /// Process-stable global config sent once via `Init` at spawn.
    global: HostGlobalConfig,
    /// Number of real subprocess spawns (incremented in `ensure_started_inner`).
    ///
    /// Doubles as a **generation counter**: `TsEngine::ensure_loaded`
    /// (`ts_engine.rs`) compares its own cached "generation as of last
    /// successful `LoadEngine`" against this value to detect that a crash
    /// respawned the subprocess out from under it — a fresh Deno process
    /// has an empty `loadedByPath`/`engineByName` (host.ts), so the module
    /// must be re-`LoadEngine`'d before the next `LaunchEngine`, even though
    /// the cached `LoadEngineResult` (static discovery metadata) itself
    /// never changes and stays cached forever. No longer `#[cfg(test)]`-only
    /// — production code reads it now, not just test assertions.
    spawn_count: AtomicU64,
    /// Number of `LoadEngine` verbs sent through `request()`.
    #[cfg(test)]
    load_engine_count: AtomicU64,
    /// Number of `MarkdownForFile` verbs sent through `request()`.
    #[cfg(test)]
    markdown_for_file_count: AtomicU64,
}

const RECENT_STDERR_CAP: usize = 100;
const CANCEL_TICK: Duration = Duration::from_millis(250);
const DISCOVERY_WINDOW: Duration = Duration::from_secs(10);

/// Maximum number of *consecutive* non-JSON stray lines `reader_loop` will
/// log-and-skip before concluding the stdout channel is genuinely
/// compromised (not just carrying one leaked banner) and falling back to the
/// kill-everything escalation. The counter resets to 0 on every well-formed
/// frame — see `reader_loop`'s `Malformed` arm for the full contract.
const MAX_CONSECUTIVE_MALFORMED_LINES: u32 = 5;

impl TsEngineHost {
    /// Construct with the process-stable global config.  Cheap — no subprocess
    /// spawn yet.
    pub fn new(global: HostGlobalConfig) -> Self {
        Self {
            write: Mutex::new(None),
            child: Arc::new(Mutex::new(None)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(0),
            reader: Mutex::new(None),
            stderr_reader: Mutex::new(None),
            recent_stderr: Arc::new(Mutex::new(VecDeque::with_capacity(RECENT_STDERR_CAP))),
            global,
            spawn_count: AtomicU64::new(0),
            #[cfg(test)]
            load_engine_count: AtomicU64::new(0),
            #[cfg(test)]
            markdown_for_file_count: AtomicU64::new(0),
        }
    }

    /// Test-only: build a host around a caller-supplied pair of halves,
    /// **bypassing subprocess spawn but still starting the real reader thread**.
    ///
    /// The `write` half goes into the same holder the live path uses; the
    /// `read` half is moved into the spawned reader thread.  `MockTransport::pair()`
    /// produces both halves.  There is no real `Child`, so `self.child` stays
    /// `None` and crash tests are driven by the mock signalling EOF.
    #[cfg(test)]
    pub fn with_transport(
        write: Arc<dyn EngineTransport>,
        read: Box<dyn EngineReadHalf>,
        global: HostGlobalConfig,
    ) -> Self {
        let host = Self::new(global);

        // Capture field-clones for the reader thread (NEVER Arc<Self>).
        let pending = Arc::clone(&host.pending);
        let child = Arc::clone(&host.child);
        let recent_stderr = Arc::clone(&host.recent_stderr);
        let shutting_down = Arc::clone(&host.shutting_down);

        let reader_handle = std::thread::spawn(move || {
            reader_loop(read, pending, child, recent_stderr, shutting_down)
        });

        // Commit: set write LAST (double-checked contract).
        *host.write.lock().unwrap() = Some(write);
        *host.reader.lock().unwrap() = Some(reader_handle);

        host
    }

    /// Test-only: build a host wired to a REAL spawned child (full reaping +
    /// stderr thread), without requiring the hardcoded deno-bundle path.
    ///
    /// Spawns `cmd` via [`spawn_into`], then calls `ensure_started_inner` with
    /// the resulting halves — so the reader thread, stderr thread, and all
    /// teardown paths are exercised exactly as in production.
    #[cfg(test)]
    pub fn start_with_command(
        cmd: Command,
        global: HostGlobalConfig,
    ) -> Result<Self, ExecutionError> {
        let host = Self::new(global);
        let child_arc = Arc::clone(&host.child);
        host.ensure_started_inner(|| {
            let (write, read, stderr) = spawn_into(cmd, child_arc)?;
            Ok((
                write as Arc<dyn EngineTransport>,
                Box::new(read) as Box<dyn EngineReadHalf>,
                Some(stderr),
            ))
        })?;
        Ok(host)
    }

    /// Lazily spawn the subprocess, start the reader + stderr threads.
    ///
    /// **Idempotent and race-safe** via double-checked commit: fast-path is
    /// `self.write.lock().unwrap().is_some()`; the coarse init lock is the
    /// `reader` mutex; committing `write` happens LAST so a failed spawn
    /// leaves `write` unset and the next call retries.
    pub fn ensure_started(&self) -> Result<(), ExecutionError> {
        self.ensure_started_inner(|| {
            if !is_available() {
                return Err(ExecutionError::other(
                    "Deno is required for TS engine extensions but was not found in PATH. \
                     Install Deno from https://deno.land/ and ensure it is on your PATH.",
                ));
            }
            let bundle_path = extracted_bundle_path()?;
            let mut cmd = Command::new("deno");
            // `--allow-all` is the ACCEPTED v1 security posture (decided 2026-07-01), not an
            // oversight. Extension bundles are third-party code running at full Deno privilege;
            // the v1 trust model is "the user installed the extension deliberately." The eventual
            // real boundary is Phase 1.6 (loopback-TCP transport + one-time token auth), not a
            // Deno permission set — so any future narrowing to `--allow-read/write/net/run` here
            // is a deliberate, separately-reviewed change, not a drive-by tightening.
            cmd.arg("run").arg("--allow-all").arg(bundle_path);
            let (write, read, stderr) = spawn_into(cmd, Arc::clone(&self.child))?;
            Ok((
                write as Arc<dyn EngineTransport>,
                Box::new(read) as Box<dyn EngineReadHalf>,
                Some(stderr),
            ))
        })
    }

    /// The double-checked init gate, parameterized over the transport source so
    /// the **race-free spawn-once** contract (seam row 16) is testable with an
    /// injected counting `init` returning mock halves — no real `deno`.
    ///
    /// `init` runs **at most once across concurrent callers**: the coarse init
    /// lock (the `reader` mutex) + the re-check under it + committing
    /// `write`'s holder LAST guarantee it. A failed `init` leaves `write`
    /// unset so the next call retries (never a cached half-init). The reader
    /// thread captures field-clones, NEVER `Arc<Self>` (spike-confirmed
    /// deadlock).
    ///
    /// Shares its coarse init lock (the `reader` mutex) with
    /// [`Self::reset_after_crash`] — a crash-triggered reset and a
    /// (re-)spawn can never interleave into a half-reset/half-init state.
    fn ensure_started_inner<F>(&self, init: F) -> Result<(), ExecutionError>
    where
        F: FnOnce() -> Result<
            (
                Arc<dyn EngineTransport>,
                Box<dyn EngineReadHalf>,
                Option<std::process::ChildStderr>,
            ),
            ExecutionError,
        >,
    {
        // Fast path — already started.
        if self.write.lock().unwrap().is_some() {
            return Ok(());
        }

        // Slow path — take the coarse init lock, re-check (double-checked).
        let mut reader_guard = self.reader.lock().unwrap();
        if self.write.lock().unwrap().is_some() {
            return Ok(());
        }

        let (write, read, stderr_opt) = init()?;

        // Record the spawn (one real spawn = one increment). Also serves as
        // the generation counter `TsEngine::ensure_loaded` keys its
        // post-crash reload decision on — see the field doc comment.
        self.spawn_count.fetch_add(1, Ordering::Relaxed);

        // Net-new production observability (J8): one INFO event per real spawn.
        // The `#[cfg(test)] spawn_count` above is invisible to integration tests
        // (they compile without `cfg(test)`), so this tracing event is the only
        // surface a project-level render can key a spawn count off — J6 asserts
        // exactly one across a two-page render, J9 asserts it orders after
        // resolution-complete. `pid` is 0 for mock-transport hosts (no real
        // `Child`); the real deno subprocess reports its OS pid.
        let pid = self.child.lock().unwrap().as_ref().map(|c| c.id());
        // NOTE: the `engine_host` target is shared with the child-stderr
        // forwarding below (reader thread). J6/J9's exactly-one-event counts
        // hold because this event fires synchronously on the caller thread
        // BEFORE the reader thread exists, and the tests' thread-local
        // subscriber never observes the reader thread. A refactor to a
        // global/dispatch subscriber would break that isolation — re-check
        // J6/J9 if you change subscriber scoping here.
        tracing::info!(target: "engine_host", pid = pid.unwrap_or(0), "engine-host spawned");

        // Spawn the stderr reader thread (real path only; mocks pass None).
        if let Some(stderr) = stderr_opt {
            let recent_stderr = Arc::clone(&self.recent_stderr);
            let stderr_handle =
                std::thread::spawn(move || stderr_loop(BufReader::new(stderr), recent_stderr));
            *self.stderr_reader.lock().unwrap() = Some(stderr_handle);
        }

        // Spawn the demux reader thread — captures field-clones, NEVER Arc<Self>.
        let pending = Arc::clone(&self.pending);
        let child_clone = Arc::clone(&self.child);
        let recent_stderr_clone = Arc::clone(&self.recent_stderr);
        let shutting_down_clone = Arc::clone(&self.shutting_down);
        let reader_handle = std::thread::spawn(move || {
            reader_loop(
                read,
                pending,
                child_clone,
                recent_stderr_clone,
                shutting_down_clone,
            )
        });
        *reader_guard = Some(reader_handle);

        // Commit: set write LAST.
        *self.write.lock().unwrap() = Some(write);

        // Fire-and-forget Init frame — sent exactly once at spawn, before any
        // LoadEngine request.  No pending slot; id u64::MAX matches Shutdown's
        // throwaway convention.
        let init_frame = Request {
            id: u64::MAX,
            msg: ToEngine::Init {
                global: self.global.clone(),
            },
        };
        let w = self.write.lock().unwrap().clone();
        if let Some(w) = w {
            let _ = w.send(&init_frame);
        }

        Ok(())
    }

    /// Reset the transport after a `ProcessCrashed` observation so the NEXT
    /// `ensure_started()` call performs a genuine fresh subprocess spawn,
    /// instead of reusing a transport whose stdin pipe is broken (the dead
    /// process's stdin).
    ///
    /// Called from `TsEngine::execute()`'s poison guard
    /// (`ts_engine.rs::execute`) when it observes
    /// `ExecutionError::ProcessCrashed` — never from the Timeout/Cancel arm,
    /// which leaves the (still-alive) transport untouched and only poisons
    /// the logical launched instance.
    ///
    /// # Generation guard (CRITICAL — concurrent shared-host safety)
    ///
    /// One `Arc<TsEngineHost>` is shared across PARALLEL document renders
    /// (`project::pass2_renderer` `docs.par_iter()` +
    /// `registry.clone()`). When the subprocess dies, `handle_crash`
    /// broadcasts `ProcessCrashed` to EVERY in-flight `PendingSlot`, so
    /// every page/chunk with an outstanding request independently reaches
    /// this method. A STALE observer can arrive *after* a sibling has
    /// already fully respawned (new healthy transport, `spawn_count`
    /// advanced). Unconditional teardown would then `take()` the NEW healthy
    /// transport, `.join()` a STILL-ALIVE reader thread (an unbounded hang
    /// while holding the coarse `reader` lock → freezing the whole host),
    /// and kill a healthy subprocess.
    ///
    /// `observed_generation` is the host's `spawn_count()` captured at the
    /// moment the crashed request was SENT (in `execute()`, before the
    /// `.request()` call) — i.e. the generation of the transport that
    /// actually crashed. The teardown is a NO-OP unless the host is STILL at
    /// that generation. The check runs under the `reader` lock, which
    /// `ensure_started_inner` also holds while it bumps `spawn_count` and
    /// commits a new transport — so "generation unchanged" atomically
    /// implies "no newer transport has been committed," and the transport we
    /// are about to tear down is genuinely the one that crashed, never a
    /// newer respawn. **Invariant: `reset_after_crash` never tears down a
    /// transport from a generation newer than the crash it responds to.**
    ///
    /// Two overlapping idempotency guards, both required:
    /// - the generation check handles the ACROSS-respawn stale observer
    ///   (its `observed_generation` is behind the current one → no-op);
    /// - the `had_transport` (`write` already `None`) check handles
    ///   SAME-generation concurrent observers (the first `take()`s the
    ///   transport; the rest see `None` and return).
    ///
    /// Takes the SAME coarse init lock (`reader`) `ensure_started_inner`
    /// uses, so a concurrent respawn can't observe a half-reset state. By
    /// the time a caller observes `ProcessCrashed`, the reader thread that
    /// delivered it has ALREADY returned (`handle_crash` runs synchronously
    /// on the reader thread immediately before `reader_loop` breaks), so the
    /// `.join()` below is instant, not a blocking wait.
    pub(crate) fn reset_after_crash(&self, observed_generation: u64) {
        let mut reader_guard = self.reader.lock().unwrap();

        // Generation guard (revert seam: this check). A stale observer whose
        // crash belongs to an OLDER generation must not touch the (newer,
        // healthy) transport a sibling already respawned. Checked under the
        // `reader` lock so it is atomic w.r.t. `ensure_started_inner`'s
        // spawn-count bump + transport commit.
        if self.spawn_count.load(Ordering::Relaxed) != observed_generation {
            return;
        }

        let had_transport = self.write.lock().unwrap().take().is_some();
        if !had_transport {
            return;
        }

        // Join the dead reader thread (already exited on EOF; instant).
        if let Some(h) = reader_guard.take() {
            let _ = h.join();
        }
        drop(reader_guard);

        if let Some(h) = self.stderr_reader.lock().unwrap().take() {
            let _ = h.join();
        }

        // Defensive: reap the child if `handle_crash` somehow left it set —
        // in the real crash path it is always already `None` (`handle_crash`
        // takes + reaps it before broadcasting), so this is normally a no-op.
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        // Clear the crash flag so the NEXT reader thread's unexpected EOF is
        // treated as a genuine crash again, not folded into an (unrelated,
        // already-handled) prior "shutting down" state.
        self.shutting_down.store(false, Ordering::Relaxed);
    }

    /// **The demux entry point.**
    ///
    /// Allocates a fresh `id`, registers a `PendingSlot` (slot-before-send
    /// ordering invariant), sends the `Request`, then blocks on the slot's
    /// `Receiver` in a tick loop polling `cancellation.is_cancelled()`.
    ///
    /// `window`:
    /// - `Some(d)` — per-request deadline; on elapse sends `Cancel{target}` and
    ///   returns `Err(Timeout{..})`.
    /// - `None` — no deadline; blocks indefinitely but still polls cancel.
    pub fn request(
        &self,
        msg: ToEngine,
        window: Option<Duration>,
        cancellation: &Cancellation,
    ) -> Result<FromEngine, ExecutionError> {
        let write = self
            .write
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| ExecutionError::other("engine host not started"))?;

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Count per-verb calls for test assertions (no-op in non-test builds).
        #[cfg(test)]
        match &msg {
            ToEngine::LoadEngine { .. } => {
                self.load_engine_count.fetch_add(1, Ordering::Relaxed);
            }
            ToEngine::MarkdownForFile { .. } => {
                self.markdown_for_file_count.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        // Extract engine name and operation for error reporting BEFORE consuming msg.
        let engine_name = engine_name_for(&msg).to_string();
        let operation = operation_name_for(&msg).to_string();

        // Create the slot BEFORE write.send (slot-ordering invariant).
        let (tx, rx) = mpsc::sync_channel::<Result<FromEngine, ExecutionError>>(1);
        self.pending.lock().unwrap().insert(
            id,
            PendingSlot {
                engine: engine_name.clone(),
                tx,
            },
        );

        // Send the request.
        let frame = Request { id, msg };
        if let Err(e) = write.send(&frame) {
            // Clean up the slot we just registered.
            self.pending.lock().unwrap().remove(&id);
            return Err(ExecutionError::other(format!("transport send error: {e}")));
        }

        // Compute the tick duration.
        let tick = match window {
            Some(w) => w.min(CANCEL_TICK),
            None => CANCEL_TICK,
        };

        let start = std::time::Instant::now();

        loop {
            if cancellation.is_cancelled() {
                // Send cooperative cancel (fire-and-forget; no slot registered).
                let cancel_id = self.next_id.fetch_add(1, Ordering::Relaxed);
                let _ = write.send(&Request {
                    id: cancel_id,
                    msg: ToEngine::Cancel { target: id },
                });
                self.pending.lock().unwrap().remove(&id);
                return Err(ExecutionError::Cancelled);
            }

            // Check window budget (only when Some).
            if let Some(w) = window
                && start.elapsed() >= w
            {
                let cancel_id = self.next_id.fetch_add(1, Ordering::Relaxed);
                let _ = write.send(&Request {
                    id: cancel_id,
                    msg: ToEngine::Cancel { target: id },
                });
                self.pending.lock().unwrap().remove(&id);
                return Err(ExecutionError::timeout(&engine_name, &operation));
            }

            match rx.recv_timeout(tick) {
                Ok(Ok(from_engine)) => {
                    return match from_engine {
                        FromEngine::Error { message, .. } => {
                            Err(ExecutionError::execution_failed(&engine_name, message))
                        }
                        other => Ok(other),
                    };
                }
                Ok(Err(e)) => return Err(e),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Tick expired — loop and re-check cancel/window.
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Reader thread exited without delivering (should not happen
                    // in the normal path — it broadcasts before exiting).
                    self.pending.lock().unwrap().remove(&id);
                    return Err(ExecutionError::other(
                        "reader thread disconnected before delivering response",
                    ));
                }
            }
        }
    }

    /// Load an engine module into the subprocess.  Discovery window: 10s.
    pub fn load_engine(
        &self,
        engine_path: &Path,
        cancellation: &Cancellation,
    ) -> Result<LoadEngineResult, ExecutionError> {
        self.ensure_started()?;
        let path_str = engine_path
            .to_str()
            .ok_or_else(|| ExecutionError::other("engine path is not valid UTF-8"))?
            .to_string();
        let msg = ToEngine::LoadEngine {
            engine_path: path_str,
        };
        let response = self.request(msg, Some(DISCOVERY_WINDOW), cancellation)?;
        match response {
            FromEngine::Loaded { discovery } => Ok(discovery),
            other => Err(ExecutionError::other(format!(
                "unexpected response to LoadEngine: {other:?}"
            ))),
        }
    }

    /// Launch an engine instance in the subprocess.  Discovery window: 10s.
    pub fn launch_engine(
        &self,
        engine: &str,
        project: EngineProjectContext,
        cancellation: &Cancellation,
    ) -> Result<LaunchEngineResult, ExecutionError> {
        self.ensure_started()?;
        let msg = ToEngine::LaunchEngine {
            engine: engine.to_string(),
            project,
        };
        let response = self.request(msg, Some(DISCOVERY_WINDOW), cancellation)?;
        match response {
            FromEngine::Launched { instance } => Ok(instance),
            other => Err(ExecutionError::other(format!(
                "unexpected response to LaunchEngine: {other:?}"
            ))),
        }
    }

    /// Graceful shutdown: close stdin, broadcast Cancelled to all pending
    /// slots, reap child, join threads.
    ///
    /// Idempotent via `Option::take()` guards on all shared handles.
    ///
    /// Note: since the `write` holder became resettable (Plan 4b F4), this
    /// now `take()`s the transport out of the `Mutex<Option<..>>` (rather
    /// than reading a `OnceLock`) and briefly holds that mutex across the
    /// `shutdown()` close. Benign: only `registry::shutdown_all` calls this,
    /// at final teardown, single-threaded — there is no concurrent
    /// `request()`/`ensure_started()` racing for the write mutex at that
    /// point.
    pub fn shutdown(&self) -> Result<(), ExecutionError> {
        // Mark shutting down before anything else.
        self.shutting_down.store(true, Ordering::Relaxed);

        // Broadcast Cancelled to all in-flight requests so they unblock.
        // Do this BEFORE closing stdin so the broadcast races the reader thread.
        {
            let slots: Vec<PendingSlot> = self
                .pending
                .lock()
                .unwrap()
                .drain()
                .map(|(_, v)| v)
                .collect();
            for slot in slots {
                let _ = slot.tx.send(Err(ExecutionError::Cancelled));
            }
        }

        // Close stdin (sends Shutdown frame then drops the BufWriter).
        if let Some(write) = self.write.lock().unwrap().take() {
            let _ = write.shutdown();
        }

        // Reap child (single-shot via take).
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.wait();
        }

        // Join reader threads.
        if let Some(handle) = self.reader.lock().unwrap().take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_reader.lock().unwrap().take() {
            let _ = handle.join();
        }

        Ok(())
    }

    /// Expose `shutting_down` for test assertions.
    #[cfg(test)]
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Relaxed)
    }

    /// True while a transport is published (post-spawn, pre-reset/shutdown).
    /// Test-only witness that `reset_after_crash`'s generation guard did NOT
    /// tear down a healthy transport.
    #[cfg(test)]
    pub fn has_transport(&self) -> bool {
        self.write.lock().unwrap().is_some()
    }

    /// True while the host holds a live child process handle.  Returns false
    /// before the subprocess is spawned and after `shutdown()`/`Drop` reaps
    /// the child.  A mock-transport host has no real `Child`, so this is
    /// always false for mocks — it reflects the REAL subprocess only.
    pub fn is_alive(&self) -> bool {
        self.child.lock().unwrap().is_some()
    }

    /// Number of real subprocess spawns recorded by `ensure_started_inner`.
    ///
    /// Also the generation counter `TsEngine::ensure_loaded` reads — see the
    /// field doc comment. No longer `#[cfg(test)]`-only.
    pub fn spawn_count(&self) -> u64 {
        self.spawn_count.load(Ordering::Relaxed)
    }

    /// Number of `LoadEngine` verbs dispatched through `request()`.
    #[cfg(test)]
    pub fn load_engine_count(&self) -> u64 {
        self.load_engine_count.load(Ordering::Relaxed)
    }

    /// Number of `MarkdownForFile` verbs dispatched through `request()`.
    #[cfg(test)]
    pub fn markdown_for_file_count(&self) -> u64 {
        self.markdown_for_file_count.load(Ordering::Relaxed)
    }
}

impl Drop for TsEngineHost {
    fn drop(&mut self) {
        // Idempotent: swap returns the OLD value; if it was already true,
        // shutdown() was called and the broadcast/take guards below are no-ops.
        self.shutting_down.swap(true, Ordering::Relaxed);

        // Broadcast Cancelled to any remaining in-flight requests.
        {
            let slots: Vec<PendingSlot> = self
                .pending
                .lock()
                .unwrap()
                .drain()
                .map(|(_, v)| v)
                .collect();
            for slot in slots {
                let _ = slot.tx.send(Err(ExecutionError::Cancelled));
            }
        }

        // Signal shutdown to the write half so the reader's recv() unblocks
        // (for mock transport: sets eof + notifies condvar; for stdio: drops stdin).
        // `get_mut` (we own `self` here, no lock contention).
        if let Some(write) = self.write.get_mut().unwrap().take() {
            let _ = write.shutdown();
        }

        // Forced kill+wait (belt-and-suspenders for panic / forgotten shutdown).
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        // Join threads via get_mut (we own self here, no lock contention).
        if let Some(handle) = self.reader.get_mut().unwrap().take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_reader.get_mut().unwrap().take() {
            let _ = handle.join();
        }
    }
}

// ============================================================================
// Reader loop (demux thread)
// ============================================================================

/// The demux reader thread body.  Captures field-clones of the host — NEVER
/// `Arc<TsEngineHost>` (spike-confirmed deadlock).
fn reader_loop(
    mut read: Box<dyn EngineReadHalf>,
    pending: Arc<Mutex<HashMap<u64, PendingSlot>>>,
    child: Arc<Mutex<Option<Child>>>,
    recent_stderr: Arc<Mutex<VecDeque<String>>>,
    shutting_down: Arc<AtomicBool>,
) {
    // Consecutive non-JSON stray lines seen since the last well-formed frame
    // (or since the reader started). See the `Malformed` arm below for the
    // full bounded-skip contract; reset to 0 whenever a frame parses.
    let mut consecutive_malformed: u32 = 0;

    loop {
        match read.recv() {
            Ok(Response { id, msg }) => {
                consecutive_malformed = 0;
                // Route by id; drop late/unknown ids silently.
                let slot = pending.lock().unwrap().remove(&id);
                if let Some(slot) = slot {
                    // Reader removes from pending BEFORE tx.send (slot-delivery
                    // invariant): capacity-1 channel; reader's send always
                    // succeeds even if the worker has abandoned its Receiver.
                    let _ = slot.tx.send(Ok(msg));
                }
            }
            Err(RecvError::Eof) => {
                if shutting_down.load(Ordering::Relaxed) {
                    // Expected exit — shutdown() closed the pipe.
                    break;
                }
                // Unexpected crash: reap code, snapshot stderr, broadcast.
                handle_crash(pending, child, recent_stderr, shutting_down);
                break;
            }
            Err(RecvError::Malformed(line)) => {
                // Bug C reader-side resilience (2026-07-02, plan
                // 2026-07-02-preview-capture-delivery.md, seam PC-C): a stray
                // non-JSON line on stdout — e.g. a leaked child banner from an
                // engine that (transiently, or due to an upstream bug) shares
                // the engine-host's stdout fd — used to be treated as proof
                // the whole channel was compromised: it killed the ENTIRE
                // subprocess and broadcast an error to EVERY pending slot,
                // discarding every in-flight capture over one leaked line
                // (the original design's finding #7, "one terminal error per
                // exit"). That was too aggressive: one stray line is not
                // evidence the channel is actually corrupted.
                //
                // New policy: log-and-skip up to
                // MAX_CONSECUTIVE_MALFORMED_LINES consecutive stray lines —
                // the counter resets on every well-formed frame (the `Ok`
                // arm above) — so a single leaked banner no longer takes down
                // the host or fails any in-flight request. Beyond the bound,
                // the channel IS treated as compromised and the original
                // kill-everything behavior fires unchanged, preserving the
                // original protection against a genuinely broken wire (wrong
                // binary spawned, protocol mismatch, a child that never stops
                // writing to the shared fd).
                //
                // Residual (documented, not fixed — ratified trade-off): if a
                // request's own response frame IS the line that gets skipped
                // here (below the bound), that request no longer gets the
                // old incidental kill-all error to wake it up. A caller that
                // sent with an explicit `timeout: false` (no window) can then
                // wait indefinitely for a response that will never arrive.
                // Callers that pass a per-request `window` timeout are
                // unaffected — they still time out on schedule; only the
                // unbounded-wait case is exposed by this change.
                consecutive_malformed += 1;
                let excerpt: String = line.chars().take(200).collect();

                if consecutive_malformed <= MAX_CONSECUTIVE_MALFORMED_LINES {
                    // Bounded log-and-skip: leave shutting_down/pending/child
                    // untouched and go back to reading the next line. Only
                    // log "skipping" here — the escalation branch below logs
                    // its own, accurate "channel considered compromised"
                    // message instead.
                    error!(
                        "engine-host protocol error: non-JSON line on stdout \
                         (likely a stray console.log/console.info in the engine, \
                         or a leaked child process's stdout — {consecutive_malformed}/\
                         {MAX_CONSECUTIVE_MALFORMED_LINES} consecutive, skipping): {excerpt:?}"
                    );
                    continue;
                }

                // Bound exceeded: no longer a single leaked banner — treat the
                // channel as genuinely compromised. Set shutting_down FIRST so
                // the kill below doesn't re-enter the crash path (finding #7
                // — one terminal error per exit).
                shutting_down.store(true, Ordering::Relaxed);

                let msg = format!(
                    "engine-host protocol error: {consecutive_malformed} consecutive \
                     non-JSON lines on stdout — channel considered compromised, \
                     terminating engine host (last line: {excerpt:?})"
                );
                error!("{}", msg);

                // Broadcast a distinct malformed-channel error.
                let slots: Vec<PendingSlot> =
                    pending.lock().unwrap().drain().map(|(_, v)| v).collect();
                for slot in slots {
                    let _ = slot.tx.send(Err(ExecutionError::other(&msg)));
                }

                // Kill the child (whole-subprocess kill for compromised channel).
                if let Some(mut c) = child.lock().unwrap().take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                break;
            }
            Err(RecvError::Io(e)) => {
                if shutting_down.load(Ordering::Relaxed) {
                    break;
                }
                error!("engine-host reader I/O error: {e}");
                handle_crash(pending, child, recent_stderr, shutting_down);
                break;
            }
        }
    }
}

/// Handle an unexpected EOF (crash): reap code, snapshot stderr, broadcast.
fn handle_crash(
    pending: Arc<Mutex<HashMap<u64, PendingSlot>>>,
    child: Arc<Mutex<Option<Child>>>,
    recent_stderr: Arc<Mutex<VecDeque<String>>>,
    shutting_down: Arc<AtomicBool>,
) {
    shutting_down.store(true, Ordering::Relaxed);

    // Reap the exit code (single-shot via take).
    let code = child
        .lock()
        .unwrap()
        .take()
        .and_then(|mut c| c.wait().ok())
        .and_then(|s| s.code());

    // Best-effort wait (~250ms) for the stderr thread to drain.
    std::thread::sleep(Duration::from_millis(250));

    // Drain pending FIRST so the roster is known when building the label.
    // Reordering drain-before-snapshot is safe: ring and pending are independent.
    let slots: Vec<PendingSlot> = pending.lock().unwrap().drain().map(|(_, v)| v).collect();

    // Build the roster of in-flight engine names (sorted + deduped for stable output).
    let mut roster: Vec<String> = slots.iter().map(|s| s.engine.clone()).collect();
    roster.sort_unstable();
    roster.dedup();

    // Snapshot the stderr ring.
    let ring_join: String = {
        let ring = recent_stderr.lock().unwrap();
        ring.iter().cloned().collect::<Vec<_>>().join("\n")
    };

    // When more than one slot was in flight, the ring is shared across engines —
    // label it honestly so callers know the tail may not originate from their engine.
    let stderr_snap = if slots.len() > 1 {
        format!(
            "recent subprocess stderr (shared across in-flight engines: [{}]):\n{}",
            roster.join(", "),
            ring_join
        )
    } else {
        ring_join
    };

    // Broadcast to every in-flight slot (each slot still carries its OWN engine name).
    for slot in slots {
        let err = ExecutionError::process_crashed(&slot.engine, code, &stderr_snap);
        let _ = slot.tx.send(Err(err));
    }
}

// ============================================================================
// Stderr reader loop
// ============================================================================

/// Drain the child's stderr, parse level prefixes, route to tracing, and fill
/// the bounded `recent_stderr` ring.
fn stderr_loop(reader: impl BufRead, recent_stderr: Arc<Mutex<VecDeque<String>>>) {
    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };

        // Route to tracing by level prefix.
        // INFO lines are trace-only: they are NOT pushed into the crash ring.
        // The skip is keyed on the literal "[INFO]" prefix, not on info-level
        // routing — bare lines also route to info! but must still be ringed.
        if let Some(rest) = line.strip_prefix("[ERROR]") {
            error!(target: "engine_host", "{}", rest.trim());
        } else if let Some(rest) = line.strip_prefix("[WARN]") {
            warn!(target: "engine_host", "{}", rest.trim());
        } else if let Some(rest) = line.strip_prefix("[INFO]") {
            info!(target: "engine_host", "{}", rest.trim());
            // INFO is chatter, not crash evidence — skip the ring push.
            continue;
        } else {
            info!(target: "engine_host", "{}", line);
        }

        // Push into bounded ring (evict oldest when full).
        let mut ring = recent_stderr.lock().unwrap();
        if ring.len() >= RECENT_STDERR_CAP {
            ring.pop_front();
        }
        ring.push_back(line);
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Extract a displayable engine name from a `ToEngine` message (for
/// `PendingSlot` and error reporting).
fn engine_name_for(msg: &ToEngine) -> &str {
    match msg {
        ToEngine::Init { .. } => "init",
        ToEngine::LoadEngine { engine_path } => engine_path.as_str(),
        ToEngine::LaunchEngine { engine, .. } => engine.as_str(),
        ToEngine::ClaimsLanguage { engine, .. } => engine.as_str(),
        ToEngine::ClaimsFile { engine, .. } => engine.as_str(),
        ToEngine::MarkdownForFile { engine, .. } => engine.as_str(),
        ToEngine::Execute { engine, .. } => engine.as_str(),
        ToEngine::IntermediateFiles { engine, .. } => engine.as_str(),
        ToEngine::Dependencies { engine, .. } => engine.as_str(),
        ToEngine::Shutdown => "shutdown",
        ToEngine::Cancel { .. } => "cancel",
    }
}

/// Return a short operation name for timeout error messages.
fn operation_name_for(msg: &ToEngine) -> &str {
    match msg {
        ToEngine::Init { .. } => "init",
        ToEngine::LoadEngine { .. } => "loadEngine",
        ToEngine::LaunchEngine { .. } => "launchEngine",
        ToEngine::ClaimsLanguage { .. } => "claimsLanguage",
        ToEngine::ClaimsFile { .. } => "claimsFile",
        ToEngine::MarkdownForFile { .. } => "markdownForFile",
        ToEngine::Execute { .. } => "execute",
        ToEngine::IntermediateFiles { .. } => "intermediateFiles",
        ToEngine::Dependencies { .. } => "dependencies",
        ToEngine::Shutdown => "shutdown",
        ToEngine::Cancel { .. } => "cancel",
    }
}

// ============================================================================
// MockTransport — test-only
// ============================================================================

#[cfg(test)]
mod mock {
    use super::*;
    use std::sync::Condvar;

    // ---- Shared inner state ----

    struct MockState {
        /// All `ToEngine` messages received by `send()`.
        sent: Vec<(u64, ToEngine)>,
        /// Scripted responses keyed by request `id`: (response, delay).
        scripted: HashMap<u64, (FromEngine, Option<Duration>)>,
        /// Queue of responses ready for the read half.
        ready: VecDeque<Response>,
        /// `true` once `signal_eof()` / `shutdown()` is called.
        eof: bool,
        /// Queue of lines to return as `RecvError::Malformed`, oldest first.
        /// A queue (not a single slot) so tests can script N *consecutive*
        /// stray lines atomically — see `signal_malformed_many` — without a
        /// delivery race against the reader thread draining them one at a time.
        malformed: VecDeque<String>,
        /// When true, automatically echo `LoadEngine` messages back as
        /// `Loaded { name: engine_path }` — avoids the pre-scripted-id
        /// assumption in concurrent tests where id assignment order is
        /// non-deterministic.
        auto_echo_load_engine: bool,
    }

    impl MockState {
        fn new() -> Self {
            Self {
                sent: Vec::new(),
                scripted: HashMap::new(),
                ready: VecDeque::new(),
                eof: false,
                malformed: VecDeque::new(),
                auto_echo_load_engine: false,
            }
        }
    }

    /// Shared inner state + condvar for blocking `recv()`.
    struct MockInner {
        state: Mutex<MockState>,
        cvar: Condvar,
    }

    // ---- Write half ----

    pub struct MockWriteHalf {
        inner: Arc<MockInner>,
    }

    impl EngineTransport for MockWriteHalf {
        fn send(&self, frame: &Request) -> Result<(), TransportError> {
            let id = frame.id;
            let mut state = self.inner.state.lock().unwrap();
            state.sent.push((id, frame.msg.clone()));

            // Auto-echo mode: immediately respond to LoadEngine with Loaded.
            if state.auto_echo_load_engine
                && let ToEngine::LoadEngine { engine_path } = &frame.msg
            {
                let response_msg = FromEngine::Loaded {
                    discovery: LoadEngineResult {
                        name: engine_path.clone(),
                        valid_extensions: vec![],
                        generates_figures: false,
                        can_freeze: false,
                        quarto_required: None,
                    },
                };
                state.ready.push_back(Response {
                    id,
                    msg: response_msg,
                });
                self.inner.cvar.notify_one();
                return Ok(());
            }

            // Fulfil any scripted response for this id.
            if let Some((response_msg, delay)) = state.scripted.remove(&id) {
                if let Some(d) = delay {
                    let inner = Arc::clone(&self.inner);
                    drop(state); // release lock before spawning
                    std::thread::spawn(move || {
                        std::thread::sleep(d);
                        let mut s = inner.state.lock().unwrap();
                        s.ready.push_back(Response {
                            id,
                            msg: response_msg,
                        });
                        inner.cvar.notify_one();
                    });
                } else {
                    state.ready.push_back(Response {
                        id,
                        msg: response_msg,
                    });
                    self.inner.cvar.notify_one();
                }
            }
            Ok(())
        }

        fn shutdown(&self) -> Result<(), TransportError> {
            let mut state = self.inner.state.lock().unwrap();
            state.eof = true;
            self.inner.cvar.notify_all();
            Ok(())
        }
    }

    impl MockWriteHalf {
        /// Enable auto-echo mode: any `LoadEngine` message is immediately
        /// answered with `Loaded { name: engine_path }`.  Use this in
        /// concurrent tests where id assignment order is non-deterministic
        /// so you can't pre-script by id.
        pub fn enable_auto_echo(&self) {
            self.inner.state.lock().unwrap().auto_echo_load_engine = true;
        }

        /// Script a response for `id` (delivered when `send()` carries that id).
        pub fn script_response(&self, id: u64, response: FromEngine) {
            self.inner
                .state
                .lock()
                .unwrap()
                .scripted
                .insert(id, (response, None));
        }

        /// Script a response with a delay.
        pub fn script_response_delayed(&self, id: u64, response: FromEngine, delay: Duration) {
            self.inner
                .state
                .lock()
                .unwrap()
                .scripted
                .insert(id, (response, Some(delay)));
        }

        /// Deliver a response to a specific id that was previously withheld.
        pub fn deliver_late(&self, id: u64, response: FromEngine) {
            let mut state = self.inner.state.lock().unwrap();
            state.ready.push_back(Response { id, msg: response });
            self.inner.cvar.notify_one();
        }

        /// Signal EOF: the read half's next `recv()` (after draining ready
        /// responses) returns `RecvError::Eof`.
        pub fn signal_eof(&self) {
            let mut state = self.inner.state.lock().unwrap();
            state.eof = true;
            self.inner.cvar.notify_all();
        }

        /// Signal a malformed line: the read half's next `recv()` returns
        /// `RecvError::Malformed(line)`.
        pub fn signal_malformed(&self, line: &str) {
            let mut state = self.inner.state.lock().unwrap();
            state.malformed.push_back(line.to_string());
            self.inner.cvar.notify_all();
        }

        /// Queue several malformed lines at once: the read half's next N
        /// `recv()` calls return `RecvError::Malformed` for each, in order,
        /// before falling through to any queued `ready` responses or EOF.
        /// All N are enqueued atomically under one lock, so — unlike calling
        /// `signal_malformed` N times — there is no race against the reader
        /// thread draining entries between calls. Used to test the reader's
        /// bounded consecutive-stray-line policy.
        pub fn signal_malformed_many(&self, lines: &[&str]) {
            let mut state = self.inner.state.lock().unwrap();
            for &line in lines {
                state.malformed.push_back(line.to_string());
            }
            self.inner.cvar.notify_all();
        }

        /// Seed crash stderr into `recent_stderr` via direct insertion.
        /// Used by crash tests to pre-populate the ring.
        pub fn seed_stderr_into(
            &self,
            recent_stderr: &Arc<Mutex<VecDeque<String>>>,
            lines: &[&str],
        ) {
            let mut ring = recent_stderr.lock().unwrap();
            for &line in lines {
                if ring.len() >= RECENT_STDERR_CAP {
                    ring.pop_front();
                }
                ring.push_back(line.to_string());
            }
        }

        /// Return all `ToEngine` messages sent so far.
        pub fn sent_messages(&self) -> Vec<ToEngine> {
            self.inner
                .state
                .lock()
                .unwrap()
                .sent
                .iter()
                .map(|(_, m)| m.clone())
                .collect()
        }
    }

    // ---- Read half ----

    pub struct MockReadHalf {
        inner: Arc<MockInner>,
    }

    impl EngineReadHalf for MockReadHalf {
        fn recv(&mut self) -> Result<Response, RecvError> {
            let mut state = self.inner.state.lock().unwrap();
            loop {
                // Check malformed first (takes priority over EOF).
                if let Some(line) = state.malformed.pop_front() {
                    return Err(RecvError::Malformed(line));
                }
                // Return next ready response if any (drain before EOF).
                if let Some(resp) = state.ready.pop_front() {
                    return Ok(resp);
                }
                // Check EOF.
                if state.eof {
                    return Err(RecvError::Eof);
                }
                // Block until something arrives.
                state = self.inner.cvar.wait(state).unwrap();
            }
        }
    }

    // ---- MockTransport ----

    /// Factory for paired mock halves.
    pub struct MockTransport;

    impl MockTransport {
        /// Create a paired (write, read) halves sharing internal state.
        pub fn pair() -> (Arc<dyn EngineTransport>, Box<dyn EngineReadHalf>) {
            let (write, read, _) = Self::pair_with_handle();
            (write, read)
        }

        /// Create a paired set AND return a typed write handle for scripting.
        pub fn pair_with_handle() -> (
            Arc<dyn EngineTransport>,
            Box<dyn EngineReadHalf>,
            Arc<MockWriteHalf>,
        ) {
            let inner = Arc::new(MockInner {
                state: Mutex::new(MockState::new()),
                cvar: Condvar::new(),
            });
            let write_typed = Arc::new(MockWriteHalf {
                inner: Arc::clone(&inner),
            });
            let write: Arc<dyn EngineTransport> =
                Arc::clone(&write_typed) as Arc<dyn EngineTransport>;
            let read: Box<dyn EngineReadHalf> = Box::new(MockReadHalf {
                inner: Arc::clone(&inner),
            });
            (write, read, write_typed)
        }
    }
}

#[cfg(test)]
pub use mock::{MockReadHalf, MockTransport, MockWriteHalf};

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::mock::MockTransport;
    use super::*;
    use std::sync::Barrier;

    fn make_host_global_config() -> HostGlobalConfig {
        HostGlobalConfig {
            resource_dir: "/res".to_string(),
            runtime_dir: "/rt".to_string(),
            data_dir: "/data".to_string(),
            pandoc_path: None,
            is_interactive_session: false,
            running_in_ci: false,
            quarto_version: "0.1.0".to_string(),
        }
    }

    fn make_load_engine_msg() -> ToEngine {
        ToEngine::LoadEngine {
            engine_path: "/engine.ts".to_string(),
        }
    }

    fn make_loaded_response(name: &str) -> FromEngine {
        FromEngine::Loaded {
            discovery: LoadEngineResult {
                name: name.to_string(),
                valid_extensions: vec![],
                generates_figures: false,
                can_freeze: false,
                quarto_required: None,
            },
        }
    }

    /// Watchdog wrapper: run `f` on a thread; panic with "DEADLOCK DETECTED" if
    /// it doesn't finish within `timeout`.
    fn watchdog<F, R>(timeout: Duration, f: F) -> R
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = f();
            let _ = tx.send(result);
        });
        rx.recv_timeout(timeout)
            .expect("DEADLOCK DETECTED: test did not complete within timeout")
    }

    // -----------------------------------------------------------------------
    // Row 3 — Concurrent id-correlation
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: `pending.remove(id)` routing in `reader_loop`.
    // Revert to delivering to ANY waiting slot → crossing → assertion fails.
    //
    // Vacuity guard: distinct payloads per engine_path ("/engine-{k}.ts");
    // identical payloads would make crossing invisible.
    //
    // Design: use auto_echo mode — the mock immediately responds to each
    // LoadEngine message with Loaded{ name: engine_path }.  This avoids the
    // pre-scripted-by-id assumption, since id assignment order is
    // non-deterministic under a barrier.  Thread k sends "/engine-k.ts" and
    // asserts the response name == "/engine-k.ts", proving its response was
    // not crossed with another thread's.
    #[test]
    fn test_concurrent_id_correlation() {
        const N: u64 = 8;

        watchdog(Duration::from_secs(15), move || {
            let (write, read, mock) = MockTransport::pair_with_handle();

            // Auto-echo: respond to any LoadEngine with Loaded{name: engine_path}.
            mock.enable_auto_echo();

            let host = Arc::new(TsEngineHost::with_transport(
                write,
                read,
                make_host_global_config(),
            ));
            let barrier = Arc::new(Barrier::new(N as usize));
            let mut handles = vec![];

            for k in 0..N {
                let host = Arc::clone(&host);
                let barrier = Arc::clone(&barrier);
                handles.push(std::thread::spawn(move || {
                    barrier.wait();
                    let cancel = Cancellation::new();
                    let path = format!("/engine-{k}.ts");
                    let result = host.request(
                        ToEngine::LoadEngine {
                            engine_path: path.clone(),
                        },
                        Some(Duration::from_secs(5)),
                        &cancel,
                    );
                    (path, result)
                }));
            }

            for handle in handles {
                let (path, result) = handle.join().unwrap();
                match result {
                    Ok(FromEngine::Loaded { discovery }) => {
                        assert_eq!(
                            discovery.name, path,
                            "id-correlation failure: request for {path} got wrong payload"
                        );
                    }
                    other => panic!("unexpected result for {path}: {other:?}"),
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Row 4 — No head-of-line blocking
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: holding pending/write mutex across the wait.
    // Revert → B cannot enter `request` while A holds the lock → B hangs.
    #[test]
    fn test_no_head_of_line_blocking() {
        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let host = Arc::new(TsEngineHost::with_transport(
                write,
                read,
                make_host_global_config(),
            ));

            let (b_done_tx, b_done_rx) = mpsc::sync_channel::<()>(1);
            let (a_done_tx, a_done_rx) = mpsc::sync_channel::<()>(1);

            // Thread A: withheld response (long window so it doesn't time out).
            let host_a = Arc::clone(&host);
            let a_handle = std::thread::spawn(move || {
                let cancel = Cancellation::new();
                let result = host_a.request(
                    make_load_engine_msg(),
                    Some(Duration::from_secs(8)),
                    &cancel,
                );
                let _ = a_done_tx.send(());
                result
            });

            // Give A a moment to register its slot (id = 0).
            std::thread::sleep(Duration::from_millis(50));

            // Thread B: response will be scripted.
            let host_b = Arc::clone(&host);
            let b_handle = std::thread::spawn(move || {
                let cancel = Cancellation::new();
                let result = host_b.request(
                    ToEngine::LoadEngine {
                        engine_path: "/engine-b.ts".to_string(),
                    },
                    Some(Duration::from_secs(8)),
                    &cancel,
                );
                let _ = b_done_tx.send(());
                result
            });

            // Give B a moment to register (id = 1).
            std::thread::sleep(Duration::from_millis(50));

            // Script B's response (id 1) — A's (id 0) remains withheld.
            mock.deliver_late(1, make_loaded_response("engine-b"));

            // B should return promptly.
            b_done_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("B did not return — possible head-of-line blocking");

            // A must still be pending (not returned).
            assert!(
                a_done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
                "A returned before its response was delivered"
            );

            // Clean up: deliver A, then JOIN both worker threads so they fully
            // unwind (drop their Arc<host> clones) before the test returns —
            // otherwise a detached worker racing teardown trips nextest's LEAK
            // grace window.
            mock.deliver_late(0, make_loaded_response("engine-a"));
            let _ = a_done_rx.recv_timeout(Duration::from_secs(5));
            let _ = a_handle.join();
            let _ = b_handle.join();
        });
    }

    // -----------------------------------------------------------------------
    // Row 5 — Timeout → distinguishable error + Cancel sent + sibling OK
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: the `recv_timeout` elapse branch in `request`.
    // Vacuity guard: assert the SPECIFIC `Timeout` variant (not just "an error").
    #[test]
    fn test_timeout_distinguishable() {
        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let host = Arc::new(TsEngineHost::with_transport(
                write,
                read,
                make_host_global_config(),
            ));

            // A: withheld, short window (id = 0).
            let host_a = Arc::clone(&host);
            let a_handle = std::thread::spawn(move || {
                let cancel = Cancellation::new();
                host_a.request(
                    make_load_engine_msg(),
                    Some(Duration::from_millis(300)), // short window
                    &cancel,
                )
            });

            // Wait for A to register, then launch B.
            std::thread::sleep(Duration::from_millis(30));

            // Script B's response (id = 1, since id 0 was A).
            mock.script_response(1, make_loaded_response("engine-b"));

            let host_b = Arc::clone(&host);
            let b_handle = std::thread::spawn(move || {
                let cancel = Cancellation::new();
                host_b.request(
                    ToEngine::LoadEngine {
                        engine_path: "/engine-b.ts".to_string(),
                    },
                    Some(Duration::from_secs(5)),
                    &cancel,
                )
            });

            let a_result = a_handle.join().unwrap();
            let b_result = b_handle.join().unwrap();

            // A must time out with the SPECIFIC Timeout variant.
            assert!(
                matches!(a_result, Err(ExecutionError::Timeout { .. })),
                "expected Timeout, got: {a_result:?}"
            );

            // Cancel{target:0} must have been sent.
            let sent = mock.sent_messages();
            let has_cancel = sent
                .iter()
                .any(|m| matches!(m, ToEngine::Cancel { target: 0 }));
            assert!(
                has_cancel,
                "Cancel{{target:0}} not in sent messages: {sent:?}"
            );

            // B must complete successfully.
            assert!(b_result.is_ok(), "B should succeed: {b_result:?}");
        });
    }

    // -----------------------------------------------------------------------
    // Row 6 — Cancel → distinguishable, prompt
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: per-tick `is_cancelled()` poll in `request`.
    // Vacuity guard: long window so timeout can't masquerade as cancel.
    #[test]
    fn test_cancel_distinguishable_and_prompt() {
        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let host = TsEngineHost::with_transport(write, read, make_host_global_config());

            let cancel = Cancellation::new();
            let cancel_clone = cancel.clone();

            // Flip the token after a short delay.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                cancel_clone.cancel();
            });

            let start = std::time::Instant::now();
            let result = host.request(
                make_load_engine_msg(),
                Some(Duration::from_secs(30)), // long window
                &cancel,
            );
            let elapsed = start.elapsed();

            assert!(
                matches!(result, Err(ExecutionError::Cancelled)),
                "expected Cancelled, got: {result:?}"
            );
            assert!(
                elapsed < Duration::from_secs(5),
                "cancel took too long ({elapsed:?}); should be ≪ 30s"
            );

            // Cancel{target:0} must have been sent (the cooperative-cancel wire).
            let sent = mock.sent_messages();
            assert!(
                sent.iter()
                    .any(|m| matches!(m, ToEngine::Cancel { target: 0 })),
                "Cancel{{target:0}} not in sent messages: {sent:?}"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Row 7 — None window still cancellable
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: `is_cancelled()` poll in the `None` branch.
    // Revert → blocks forever → watchdog fires RED.
    #[test]
    fn test_none_window_still_cancellable() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, _mock) = MockTransport::pair_with_handle();
            let host = TsEngineHost::with_transport(write, read, make_host_global_config());

            let cancel = Cancellation::new();
            let cancel_clone = cancel.clone();

            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                cancel_clone.cancel();
            });

            let result = host.request(
                make_load_engine_msg(),
                None, // no deadline
                &cancel,
            );

            assert!(
                matches!(result, Err(ExecutionError::Cancelled)),
                "expected Cancelled with None window, got: {result:?}"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Row 8 — Abandoned-slot doesn't wedge the demux
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: the graceful drop of orphaned late responses in
    // `reader_loop` (the `if let Some(slot)` guard). Revert it to
    // `pending.remove(id).unwrap()` → the reader panics on A's late (orphaned)
    // response → poisons `pending` → B hangs → watchdog RED. (fail-on-revert
    // verified 2026-06-24.)
    //
    // NOTE: the frozen seam spec named a `sync_channel(1)→(0)` revert here; that
    // revert does NOT redden, so it is corrected to the one above. When A times
    // out it DROPS its `Receiver`, and a std `sync_channel(0)` sender returns
    // `Err` (it does NOT block) on a dropped receiver, so the reader never wedges.
    // `sync_channel(1)` remains the correct defensive choice (the single reader
    // must never block delivering, even transiently — that would head-of-line
    // stall other slots), but the property THIS test discriminates is the
    // graceful orphan-drop above.
    #[test]
    fn test_abandoned_slot_does_not_wedge_demux() {
        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let host = Arc::new(TsEngineHost::with_transport(
                write,
                read,
                make_host_global_config(),
            ));

            // A: short window → times out, abandons slot (id = 0).
            let host_a = Arc::clone(&host);
            let a_handle = std::thread::spawn(move || {
                let cancel = Cancellation::new();
                host_a.request(
                    make_load_engine_msg(),
                    Some(Duration::from_millis(200)),
                    &cancel,
                )
            });

            let a_result = a_handle.join().unwrap();
            assert!(
                matches!(a_result, Err(ExecutionError::Timeout { .. })),
                "A should timeout: {a_result:?}"
            );

            // A's Cancel was id 1; next free id is 2.
            // Late-deliver A's original response (id 0): reader must drop it
            // (slot removed), not block.
            mock.deliver_late(0, make_loaded_response("engine-a-late"));

            // Small pause to let the reader process the late delivery.
            std::thread::sleep(Duration::from_millis(50));

            // Script B's response (id 2).
            mock.script_response(2, make_loaded_response("engine-b"));

            let b_result = host.request(
                ToEngine::LoadEngine {
                    engine_path: "/engine-b.ts".to_string(),
                },
                Some(Duration::from_secs(5)),
                &Cancellation::new(),
            );

            assert!(
                b_result.is_ok(),
                "B should succeed after abandoned slot: {b_result:?}"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Row 9 — Crash broadcast (mock EOF)
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: drain-and-broadcast in `handle_crash`.
    // Revert → reader just exits → N requests hang → watchdog fires RED.
    #[test]
    fn test_crash_broadcast_on_mock_eof() {
        const N: usize = 4;

        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();

            let host = Arc::new(TsEngineHost::with_transport(
                write,
                read,
                make_host_global_config(),
            ));

            // Seed crash stderr into the host's recent_stderr ring.
            mock.seed_stderr_into(&host.recent_stderr, &["fatal error from stderr"]);

            // Spawn N workers — all withheld.
            let barrier = Arc::new(Barrier::new(N));
            let mut handles = vec![];
            for i in 0..N {
                let host = Arc::clone(&host);
                let barrier = Arc::clone(&barrier);
                handles.push(std::thread::spawn(move || {
                    barrier.wait();
                    let cancel = Cancellation::new();
                    host.request(
                        ToEngine::LoadEngine {
                            engine_path: format!("/engine-{i}.ts"),
                        },
                        Some(Duration::from_secs(10)),
                        &cancel,
                    )
                }));
            }

            // Wait for all workers to register their slots.
            std::thread::sleep(Duration::from_millis(100));

            // Signal EOF.
            mock.signal_eof();

            // Every worker must get ProcessCrashed WITH the seeded stderr tail
            // attached (not just the variant).
            for (i, handle) in handles.into_iter().enumerate() {
                match handle.join().unwrap() {
                    Err(ExecutionError::ProcessCrashed { stderr, .. }) => {
                        assert!(
                            stderr.contains("fatal error from stderr"),
                            "worker {i} ProcessCrashed missing seeded stderr: {stderr:?}"
                        );
                    }
                    other => panic!("worker {i} should get ProcessCrashed, got: {other:?}"),
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Row 10a — Stray lines below the bound: log-and-skip, not fatal
    // -----------------------------------------------------------------------
    //
    // Bug C resilience (plan 2026-07-02-preview-capture-delivery.md, seam
    // PC-C, resilience leg): a stray non-JSON line on the shared engine-host
    // stdout — e.g. a leaked child banner — must NOT kill the host and must
    // NOT fail an UNRELATED in-flight request; the real response each request
    // is waiting on must still be delivered to its own slot afterwards.
    //
    // Named revert hunk: the bounded log-and-skip early-continue in the
    // `RecvError::Malformed` arm of `reader_loop`. Revert it → the first
    // stray line broadcasts an error to every pending slot and kills the
    // channel → both A and B fail immediately with the malformed-channel
    // error instead of their real responses → RED.
    #[test]
    fn test_stray_lines_below_bound_are_skipped_not_fatal() {
        watchdog(Duration::from_secs(15), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let host = Arc::new(TsEngineHost::with_transport(
                write,
                read,
                make_host_global_config(),
            ));

            // A and B: two unrelated in-flight requests (ids 0 and 1).
            let host_a = Arc::clone(&host);
            let a_handle = std::thread::spawn(move || {
                let cancel = Cancellation::new();
                host_a.request(
                    make_load_engine_msg(),
                    Some(Duration::from_secs(10)),
                    &cancel,
                )
            });
            std::thread::sleep(Duration::from_millis(30));

            let host_b = Arc::clone(&host);
            let b_handle = std::thread::spawn(move || {
                let cancel = Cancellation::new();
                host_b.request(
                    ToEngine::LoadEngine {
                        engine_path: "/engine-b.ts".to_string(),
                    },
                    Some(Duration::from_secs(10)),
                    &cancel,
                )
            });
            std::thread::sleep(Duration::from_millis(30));

            // A couple of stray non-JSON lines (below the bound) — must be
            // logged and skipped, not escalated.
            mock.signal_malformed_many(&[
                "[ Info: Log started at 2026-07-02T13:11:20.379",
                "console.log('another stray line')",
            ]);

            // Let the reader drain both stray lines before the real
            // responses arrive.
            std::thread::sleep(Duration::from_millis(50));

            // Both A and B's real responses still arrive normally,
            // out of order, via deliver_late (both requests' `send()`
            // already happened before we could pre-script by id).
            mock.deliver_late(1, make_loaded_response("engine-b"));
            mock.deliver_late(0, make_loaded_response("engine-a"));

            let a_result = a_handle.join().unwrap();
            let b_result = b_handle.join().unwrap();

            assert!(
                matches!(a_result, Ok(FromEngine::Loaded { .. })),
                "A must still receive its own response after unrelated stray lines: {a_result:?}"
            );
            assert!(
                matches!(b_result, Ok(FromEngine::Loaded { .. })),
                "B (unrelated in-flight request) must not fail because of stray lines \
                 on the shared channel: {b_result:?}"
            );
            assert!(
                !host.is_shutting_down(),
                "a below-bound run of stray lines must not tear down the channel"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Row 10b — Beyond the bound: still escalates, still distinct from crash
    // -----------------------------------------------------------------------
    //
    // The bound is enforced, not decorative: MAX_CONSECUTIVE_MALFORMED_LINES+1
    // consecutive stray lines (no valid frame in between to reset the
    // counter) must still fall back to the pre-existing kill-everything
    // behavior — broadcast an `Other` error (NOT `ProcessCrashed` — this is a
    // distinct failure mode from a real subprocess crash) and mark
    // `shutting_down`.
    //
    // Named revert hunk: the escalation branch (`consecutive_malformed >
    // MAX_CONSECUTIVE_MALFORMED_LINES`) in the `RecvError::Malformed` arm.
    // Revert it to unconditional skip → this request never resolves →
    // watchdog fires RED (the bound would be decorative).
    #[test]
    fn test_malformed_beyond_bound_escalates_distinct_from_crash() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            let host = Arc::new(TsEngineHost::with_transport(
                write,
                read,
                make_host_global_config(),
            ));

            let host_req = Arc::clone(&host);
            let request_handle = std::thread::spawn(move || {
                let cancel = Cancellation::new();
                host_req.request(
                    make_load_engine_msg(),
                    Some(Duration::from_secs(10)),
                    &cancel,
                )
            });

            // Let the request register.
            std::thread::sleep(Duration::from_millis(50));

            // One MORE than the bound of consecutive stray lines, with no
            // valid frame in between to reset the counter.
            let lines: Vec<String> = (0..=MAX_CONSECUTIVE_MALFORMED_LINES)
                .map(|i| format!("console.log('stray {i}')"))
                .collect();
            let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            mock.signal_malformed_many(&line_refs);

            let result = request_handle.join().unwrap();

            // Must be Other(_), NOT ProcessCrashed.
            assert!(
                matches!(result, Err(ExecutionError::Other(_))),
                "expected Other, got: {result:?}"
            );

            // shutting_down must be set once the bound is exceeded.
            assert!(
                host.is_shutting_down(),
                "shutting_down should be set once the consecutive-stray-line bound is exceeded"
            );
        });
    }

    // -----------------------------------------------------------------------
    // H1-a — crash names shared roster when >1 in-flight
    // -----------------------------------------------------------------------
    //
    // Named revert: remove the `slots.len() > 1` label branch (always emit
    // bare ring_join) → "alpha"/"shared" absent from stderr → RED.
    // `glitch` is the path-exercised assertion: confirms the ring snapshot is
    // still attached even when the label is present.
    #[test]
    fn test_crash_shared_roster_label() {
        watchdog(Duration::from_secs(5), || {
            // Pre-fill ring with an engine-name-free WARN line.
            let recent_stderr: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
            recent_stderr
                .lock()
                .unwrap()
                .push_back("[WARN] glitch".to_string());

            // Two in-flight slots: alpha + beta.
            let (tx_alpha, rx_alpha) = mpsc::sync_channel::<Result<FromEngine, ExecutionError>>(1);
            let (tx_beta, rx_beta) = mpsc::sync_channel::<Result<FromEngine, ExecutionError>>(1);
            let mut pending_map: HashMap<u64, PendingSlot> = HashMap::new();
            pending_map.insert(
                1u64,
                PendingSlot {
                    engine: "alpha".to_string(),
                    tx: tx_alpha,
                },
            );
            pending_map.insert(
                2u64,
                PendingSlot {
                    engine: "beta".to_string(),
                    tx: tx_beta,
                },
            );
            let pending = Arc::new(Mutex::new(pending_map));
            let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
            let shutting_down = Arc::new(AtomicBool::new(false));

            handle_crash(pending, child, recent_stderr, shutting_down);

            // Both receivers must see ProcessCrashed with honest shared label.
            for (name, rx) in [("alpha_rx", rx_alpha), ("beta_rx", rx_beta)] {
                match rx.recv().unwrap() {
                    Err(ExecutionError::ProcessCrashed { stderr, .. }) => {
                        assert!(
                            stderr.contains("shared"),
                            "{name}: crash stderr should contain 'shared': {stderr:?}"
                        );
                        assert!(
                            stderr.contains("alpha"),
                            "{name}: crash stderr should contain 'alpha': {stderr:?}"
                        );
                        assert!(
                            stderr.contains("glitch"),
                            "{name}: crash stderr should contain 'glitch': {stderr:?}"
                        );
                    }
                    other => panic!("{name}: expected ProcessCrashed, got: {other:?}"),
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // H1-c — INFO lines not pushed into ring; WARN/ERROR/bare are
    // -----------------------------------------------------------------------
    //
    // Named reverts:
    //   (1) remove [INFO]-skip guard → `!ring.contains("hello")` RED
    //   (2) broaden skip to all `[`-prefixed lines → "careful" absent → RED
    //   (3) key skip on info-level routing rather than literal [INFO] prefix
    //       (bare lines also route to info!) → "bare" absent → RED
    #[test]
    fn test_stderr_loop_info_not_ringed() {
        let ring: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
        let input = b"[INFO] hello\n[WARN] careful\n[ERROR] boom\nbare\n";
        stderr_loop(std::io::Cursor::new(&input[..]), Arc::clone(&ring));
        let contents: Vec<String> = ring.lock().unwrap().iter().cloned().collect();

        assert!(
            !contents.iter().any(|l| l.contains("hello")),
            "[INFO] hello should NOT be in ring, but got: {contents:?}"
        );
        assert!(
            contents.iter().any(|l| l.contains("careful")),
            "[WARN] careful should be in ring: {contents:?}"
        );
        assert!(
            contents.iter().any(|l| l.contains("boom")),
            "[ERROR] boom should be in ring: {contents:?}"
        );
        assert!(
            contents.iter().any(|l| l.contains("bare")),
            "bare line should be in ring: {contents:?}"
        );
    }

    // -----------------------------------------------------------------------
    // H1-b — single in-flight slot: no shared label, ring body preserved
    // -----------------------------------------------------------------------
    //
    // Named revert: drop the `slots.len() > 1` guard (label unconditional) →
    // single-slot path shows "shared across" → `!stderr.contains("shared")` RED.
    // `contains("x")` is the path-exercised co-assertion: reddens a fix that
    // returns empty/contentless stderr on the solo path.
    #[test]
    fn test_crash_single_slot_no_shared_label() {
        watchdog(Duration::from_secs(5), || {
            let recent_stderr: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
            recent_stderr
                .lock()
                .unwrap()
                .push_back("[WARN] x".to_string());

            let (tx_solo, rx_solo) = mpsc::sync_channel::<Result<FromEngine, ExecutionError>>(1);
            let mut pending_map: HashMap<u64, PendingSlot> = HashMap::new();
            pending_map.insert(
                1u64,
                PendingSlot {
                    engine: "solo".to_string(),
                    tx: tx_solo,
                },
            );
            let pending = Arc::new(Mutex::new(pending_map));
            let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
            let shutting_down = Arc::new(AtomicBool::new(false));

            handle_crash(pending, child, recent_stderr, shutting_down);

            match rx_solo.recv().unwrap() {
                Err(ExecutionError::ProcessCrashed { stderr, .. }) => {
                    assert!(
                        !stderr.contains("shared"),
                        "single-slot crash should not label stderr as shared: {stderr:?}"
                    );
                    assert!(
                        stderr.contains('x'),
                        "single-slot crash stderr should still carry ring body: {stderr:?}"
                    );
                }
                other => panic!("expected ProcessCrashed, got: {other:?}"),
            }
        });
    }

    // -----------------------------------------------------------------------
    // Row 16 — Race-free ensure_started (spawn count == 1)
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: the coarse init lock (reader mutex) in `ensure_started`.
    // Revert → two concurrent callers both pass the fast-path check → both
    // spawn → count == 2 → assertion fails.
    //
    // Drives the REAL `TsEngineHost::ensure_started_inner` gate (the production
    // code under test) with an injected counting `init` returning mock halves —
    // NOT a toy OnceLock. Two barrier-aligned callers; the coarse init lock must
    // let `init` run exactly once. Reverting the coarse lock makes both callers
    // run `init` → count == 2 → RED.
    #[test]
    fn test_race_free_ensure_started() {
        use std::sync::atomic::AtomicUsize;

        watchdog(Duration::from_secs(10), || {
            let host = Arc::new(TsEngineHost::new(make_host_global_config()));
            let spawn_count = Arc::new(AtomicUsize::new(0));
            let barrier = Arc::new(Barrier::new(2));

            let mut handles = vec![];
            for _ in 0..2 {
                let host = Arc::clone(&host);
                let spawn_count = Arc::clone(&spawn_count);
                let barrier = Arc::clone(&barrier);
                handles.push(std::thread::spawn(move || {
                    barrier.wait();
                    host.ensure_started_inner(|| {
                        spawn_count.fetch_add(1, Ordering::Relaxed);
                        let (write, read) = MockTransport::pair();
                        Ok((write, read, None))
                    })
                }));
            }

            for h in handles {
                h.join().unwrap().expect("ensure_started_inner failed");
            }

            assert_eq!(
                spawn_count.load(Ordering::Relaxed),
                1,
                "init must run exactly once across concurrent callers"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Plan 4b F4 review — reset_after_crash generation guard (CRITICAL)
    // -----------------------------------------------------------------------
    //
    // One `Arc<TsEngineHost>` is shared across parallel document renders, so
    // a crash broadcasts `ProcessCrashed` to every in-flight request and each
    // reaches `reset_after_crash` independently. A STALE observer (its crash
    // belongs to an OLD generation) can arrive AFTER a sibling already
    // respawned a fresh, HEALTHY transport. Without the generation guard, the
    // stale observer would tear down that healthy transport and `.join()` its
    // STILL-ALIVE reader thread — an unbounded hang while holding the coarse
    // `reader` lock, freezing the whole host.
    //
    // This test is deterministic (no sleeps/timing races on the pass path):
    // it advances the generation explicitly via `ensure_started_inner`, then
    // invokes `reset_after_crash` with a STALE generation and asserts it is a
    // NO-OP — the healthy transport is NOT taken.
    //
    // revert seam: the generation check in `reset_after_crash`
    // (`if self.spawn_count.load(..) != observed_generation { return; }`).
    // Reverting it makes the stale reset take the healthy gen-2 transport and
    // then block forever joining gen-2's live reader thread → the watchdog
    // fires ("DEADLOCK DETECTED") → RED. (Full-crate revert-verified.)
    #[test]
    fn test_reset_after_crash_generation_guard_ignores_stale_observer() {
        watchdog(Duration::from_secs(10), || {
            let host = TsEngineHost::new(make_host_global_config());

            // --- Generation 1: publish a mock transport (reader thread runs).
            let (w1, r1, mock1) = MockTransport::pair_with_handle();
            host.ensure_started_inner(|| Ok((w1, r1, None)))
                .expect("gen-1 spawn");
            assert_eq!(host.spawn_count(), 1);
            assert!(host.has_transport());

            // Simulate the gen-1 subprocess crashing: the reader hits EOF and
            // exits (handle_crash runs on the reader thread), exactly as after
            // a real crash — so the legitimate reset's `.join()` is instant.
            mock1.signal_eof();

            // Legitimate reset for the crash observed at generation 1.
            host.reset_after_crash(1);
            assert!(
                !host.has_transport(),
                "the gen-1 crash reset must clear the (dead) transport"
            );
            assert_eq!(host.spawn_count(), 1, "reset itself never spawns");

            // --- Generation 2: a sibling respawns a fresh, HEALTHY transport
            // (its reader thread is alive and parked on recv()).
            let (w2, r2, mock2) = MockTransport::pair_with_handle();
            host.ensure_started_inner(|| Ok((w2, r2, None)))
                .expect("gen-2 respawn");
            assert_eq!(host.spawn_count(), 2);
            assert!(
                host.has_transport(),
                "the gen-2 transport is published and healthy"
            );

            // --- THE CRITICAL CASE: a STALE observer of the OLD gen-1 crash
            // now calls reset with observed_generation = 1. It MUST be a
            // no-op: the healthy gen-2 transport must NOT be torn down, its
            // live reader must NOT be joined (a join here would hang, caught
            // by the watchdog), no child killed.
            host.reset_after_crash(1);
            assert!(
                host.has_transport(),
                "STALE reset (observed gen 1) must NOT tear down the healthy \
                 gen-2 transport"
            );
            assert_eq!(host.spawn_count(), 2);

            // Same-generation observer safety: a reset AT the current
            // generation still tears the crashed transport down. Simulate the
            // gen-2 crash first so its reader exits and the join is instant.
            mock2.signal_eof();
            host.reset_after_crash(2);
            assert!(
                !host.has_transport(),
                "a current-generation reset tears down the crashed transport"
            );

            // host `Drop` joins any remaining threads cleanly.
        });
    }

    // -----------------------------------------------------------------------
    // J8 — engine-host spawn observability event (Plan 4H / Test Seam Spec J8)
    // -----------------------------------------------------------------------
    //
    // Net-new PRODUCTION line: `tracing::info!(target: "engine_host", pid, …)`
    // in `ensure_started_inner`, fired once per REAL spawn. Integration tests
    // (J6/J9) compile WITHOUT `cfg(test)`, so the `#[cfg(test)] spawn_count`
    // counter above is invisible to them — this tracing event is the only
    // observable surface a project-level render can key off.
    //
    // Named revert (Test Seam Spec J8): remove the `tracing::info!` line in
    // `ensure_started_inner` → the capture sees zero `engine_host` events →
    // both of these unit rows AND the J6/J9 integration captures go RED.

    /// A minimal tracing-capture layer recording the `target` of every event,
    /// in arrival order, shared across threads via `Arc<Mutex<Vec<String>>>`.
    #[derive(Clone, Default)]
    struct TargetCapture {
        targets: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for TargetCapture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            self.targets
                .lock()
                .unwrap()
                .push(event.metadata().target().to_string());
        }
    }

    impl TargetCapture {
        fn count(&self, target: &str) -> usize {
            self.targets
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.as_str() == target)
                .count()
        }
    }

    // J8-a: exactly one `engine_host` event per real spawn; the idempotent
    // second `ensure_started_inner` (fast-path, no init) fires NONE.
    #[test]
    fn test_j8_spawn_event_fires_once_per_spawn() {
        use tracing_subscriber::layer::SubscriberExt;
        watchdog(Duration::from_secs(10), || {
            let host = TsEngineHost::new(make_host_global_config());
            let capture = TargetCapture::default();
            let subscriber = tracing_subscriber::registry().with(capture.clone());
            tracing::subscriber::with_default(subscriber, || {
                host.ensure_started_inner(|| {
                    let (write, read) = MockTransport::pair();
                    Ok((write, read, None))
                })
                .expect("first ensure_started_inner");
                // Idempotent: write is already committed → init never runs →
                // no second spawn event.
                host.ensure_started_inner(|| {
                    let (write, read) = MockTransport::pair();
                    Ok((write, read, None))
                })
                .expect("second ensure_started_inner (idempotent)");
            });
            assert_eq!(
                capture.count("engine_host"),
                1,
                "exactly one engine_host spawn event per real spawn (the idempotent \
                 second call must fire none)"
            );
        });
    }

    // J8-b: exactly one `engine_host` event under the concurrent-spawn Barrier
    // race — init runs once (coarse lock), so the event fires once. A shared
    // `Dispatch` is installed on BOTH spawn threads (the event fires on
    // whichever thread wins the init) so the capture sees it regardless.
    #[test]
    fn test_j8_spawn_event_once_under_concurrent_spawn() {
        use tracing_subscriber::layer::SubscriberExt;
        watchdog(Duration::from_secs(10), || {
            let host = Arc::new(TsEngineHost::new(make_host_global_config()));
            let capture = TargetCapture::default();
            let dispatch =
                tracing::Dispatch::new(tracing_subscriber::registry().with(capture.clone()));
            let barrier = Arc::new(Barrier::new(2));

            let mut handles = vec![];
            for _ in 0..2 {
                let host = Arc::clone(&host);
                let barrier = Arc::clone(&barrier);
                let dispatch = dispatch.clone();
                handles.push(std::thread::spawn(move || {
                    tracing::dispatcher::with_default(&dispatch, || {
                        barrier.wait();
                        host.ensure_started_inner(|| {
                            let (write, read) = MockTransport::pair();
                            Ok((write, read, None))
                        })
                    })
                }));
            }
            for h in handles {
                h.join().unwrap().expect("ensure_started_inner failed");
            }

            assert_eq!(
                capture.count("engine_host"),
                1,
                "exactly one engine_host spawn event even under concurrent spawn \
                 (init runs once behind the coarse lock)"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Task 3 — TsEngineHost observability: is_alive + per-verb/spawn counters
    // -----------------------------------------------------------------------

    // P3-1a: freshly constructed host has no child → is_alive() == false.
    // This assertion is unconditional (no Deno gate) — the method must exist
    // and return false before any subprocess is started.
    //
    // Named revert hunk: `is_alive()` method on `TsEngineHost`.
    // Revert → compile error ("no method named `is_alive`") → RED.
    #[test]
    fn test_is_alive_new_false() {
        let host = TsEngineHost::new(make_host_global_config());
        assert!(
            !host.is_alive(),
            "freshly constructed host must not be alive (child is None)"
        );
    }

    // P3-2: load_engine_count increments once per LoadEngine verb via mock.
    // Also confirms spawn_count stays 0 (mock has no real child process).
    //
    // Named revert hunk: `#[cfg(test)] load_engine_count` increment in `request`.
    // Revert → count stays 0 → `load_engine_count() == 1` assertion fails RED.
    #[test]
    fn test_load_engine_count_via_mock() {
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            // Auto-echo: any LoadEngine is answered with Loaded immediately.
            mock.enable_auto_echo();
            let host = TsEngineHost::with_transport(write, read, make_host_global_config());
            let cancel = Cancellation::new();
            let result = host.load_engine(Path::new("/engine.ts"), &cancel);
            assert!(result.is_ok(), "load_engine should succeed: {result:?}");
            assert_eq!(
                host.load_engine_count(),
                1,
                "one LoadEngine request must increment load_engine_count to 1"
            );
            assert_eq!(
                host.spawn_count(),
                0,
                "mock-transport host has no real subprocess; spawn_count must stay 0"
            );
        });
    }

    // P3-3: markdown_for_file_count increments once per MarkdownForFile verb.
    //
    // Named revert hunk: `#[cfg(test)] markdown_for_file_count` increment in
    // `request`. Revert → count stays 0 → assertion fails RED.
    #[test]
    fn test_markdown_for_file_count_via_mock() {
        use crate::engine::ts_protocol::TsMappedStringWithMap;
        watchdog(Duration::from_secs(10), || {
            let (write, read, mock) = MockTransport::pair_with_handle();
            // Script MarkdownForFileResult for id 0 (first request on this host).
            mock.script_response(
                0,
                FromEngine::MarkdownForFileResult {
                    result: TsMappedStringWithMap {
                        value: "output".to_string(),
                        file_name: None,
                        source_map: vec![],
                    },
                },
            );
            let host = TsEngineHost::with_transport(write, read, make_host_global_config());
            let cancel = Cancellation::new();
            let msg = ToEngine::MarkdownForFile {
                engine: "my_engine".to_string(),
                file: "/test.ts".to_string(),
            };
            let _ = host.request(msg, Some(Duration::from_secs(5)), &cancel);
            assert_eq!(
                host.markdown_for_file_count(),
                1,
                "one MarkdownForFile request must increment markdown_for_file_count to 1"
            );
        });
    }
}

// ============================================================================
// Proc-tier tests (real child process, Unix-only — uses /bin/sleep, sh -c,
// signals; watchdog-wrapped).
// ============================================================================

#[cfg(all(test, unix))]
mod proc_tests {
    use super::*;
    use std::sync::mpsc;

    fn make_host_global_config() -> HostGlobalConfig {
        HostGlobalConfig {
            resource_dir: "/res".to_string(),
            runtime_dir: "/rt".to_string(),
            data_dir: "/data".to_string(),
            pandoc_path: None,
            is_interactive_session: false,
            running_in_ci: false,
            quarto_version: "0.1.0".to_string(),
        }
    }

    /// Watchdog wrapper (mirrors the one in `tests`): run `f` on a thread;
    /// panic with "DEADLOCK DETECTED" if it doesn't finish within `timeout`.
    fn watchdog<F, R>(timeout: Duration, f: F) -> R
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = f();
            let _ = tx.send(result);
        });
        rx.recv_timeout(timeout)
            .expect("DEADLOCK DETECTED: test did not complete within timeout")
    }

    // -----------------------------------------------------------------------
    // Row 11 — Drop reaps, no hang
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: field-clones capture in reader thread (vs Arc<Self>).
    // Revert Arc<Self> → Drop never runs → child PID survives → assertion fails.
    //
    // Child: `sleep 60` — ignores stdin, never exits on its own.
    // Assert: (a) drop returned within bound and (b) PID is gone (kill -0 fails).
    #[test]
    fn test_drop_reaps_no_hang() {
        watchdog(Duration::from_secs(10), || {
            let mut cmd = Command::new("sleep");
            cmd.arg("60");
            let host = TsEngineHost::start_with_command(cmd, make_host_global_config())
                .expect("start_with_command failed");

            // Capture PID before dropping.
            let pid = host.child.lock().unwrap().as_ref().unwrap().id();

            let start = std::time::Instant::now();
            drop(host);
            let elapsed = start.elapsed();

            // (a) Drop returned promptly.
            assert!(
                elapsed < Duration::from_secs(5),
                "drop took too long ({elapsed:?}); possible hang in teardown"
            );

            // (b) PID is gone — kill -0 should fail with ESRCH.
            let status = Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .status()
                .expect("failed to run kill -0");
            assert!(
                !status.success(),
                "PID {pid} is still alive after drop — child was not reaped"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Row 12 — Graceful shutdown() joins
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: `reader.lock().take().join()` / `write.take()` close
    // in `shutdown`.  Revert → threads not joined / stdin not closed → hang.
    //
    // Child: `sh -c 'cat >/dev/null'` — reads+discards stdin, exits 0 on EOF.
    #[test]
    fn test_graceful_shutdown_joins() {
        watchdog(Duration::from_secs(10), || {
            let host = TsEngineHost::start_with_command(
                {
                    let mut cmd = Command::new("sh");
                    cmd.arg("-c").arg("cat >/dev/null");
                    cmd
                },
                make_host_global_config(),
            )
            .expect("start_with_command failed");

            let result = host.shutdown();
            assert!(
                result.is_ok(),
                "shutdown() should return Ok, got: {result:?}"
            );
            // If we reach here without the watchdog firing, threads joined cleanly.
        });
    }

    // -----------------------------------------------------------------------
    // Row 13 — Double-teardown idempotent
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: `Option::take()` single-shot guards on child/handles.
    // Revert plain value → shutdown then drop double-wait/join → panic or UB.
    //
    // Child: same `sh -c 'cat >/dev/null'`.
    #[test]
    fn test_double_teardown_idempotent() {
        watchdog(Duration::from_secs(10), || {
            let host = TsEngineHost::start_with_command(
                {
                    let mut cmd = Command::new("sh");
                    cmd.arg("-c").arg("cat >/dev/null");
                    cmd
                },
                make_host_global_config(),
            )
            .expect("start_with_command failed");

            // First teardown: explicit shutdown.
            let _ = host.shutdown();
            // Second teardown: implicit via Drop.  Must not panic or hang.
            drop(host);
            // Reaching here without the watchdog = idempotent. ✓
        });
    }

    // -----------------------------------------------------------------------
    // Row 14 — Real multiplex smoke (deno-gated; SKIPPED if deno not on PATH)
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: newline-delimiter framing in StdioWriteHalf/StdioReadHalf.
    // Revert framing → N distinct ids fail to round-trip → assertion fails.
    //
    // SKIPPED in CI (no deno installed).
    #[test]
    fn test_real_multiplex_smoke_deno() {
        // Skip if deno is not on PATH.
        if !is_available() {
            return;
        }

        watchdog(Duration::from_secs(30), || {
            use std::io::Write as _;
            use tempfile::NamedTempFile;

            // Write a tiny deno echo script: reads Request lines from stdin,
            // echoes Loaded{name: enginePath} back for each LoadEngine.
            let script = r#"
const dec = new TextDecoder();
const enc = new TextEncoder();
const buf = new Uint8Array(1024 * 64);
let pending = "";
async function readLines() {
  while (true) {
    const n = await Deno.stdin.read(buf);
    if (n === null) break;
    pending += dec.decode(buf.subarray(0, n));
    let idx;
    while ((idx = pending.indexOf("\n")) !== -1) {
      const line = pending.slice(0, idx).trim();
      pending = pending.slice(idx + 1);
      if (!line) continue;
      const req = JSON.parse(line);
      if (req.msg && req.msg.type === "loadEngine") {
        const resp = JSON.stringify({
          id: req.id,
          msg: {
            type: "loaded",
            discovery: { name: req.msg.enginePath, validExtensions: [] }
          }
        }) + "\n";
        await Deno.stdout.write(enc.encode(resp));
      } else if (req.msg && req.msg.type === "shutdown") {
        Deno.exit(0);
      }
    }
  }
}
await readLines();
"#;
            let mut tmp = NamedTempFile::new().expect("tempfile");
            tmp.write_all(script.as_bytes()).expect("write script");
            let script_path = tmp.path().to_path_buf();

            let mut cmd = Command::new("deno");
            cmd.arg("run").arg("--allow-all").arg(&script_path);

            let host = Arc::new(
                TsEngineHost::start_with_command(cmd, make_host_global_config())
                    .expect("start_with_command failed"),
            );

            const N: u64 = 4;
            let mut handles = vec![];
            for k in 0..N {
                let host = Arc::clone(&host);
                handles.push(std::thread::spawn(move || {
                    let cancel = Cancellation::new();
                    let path = format!("/engine-{k}.ts");
                    let result = host.request(
                        ToEngine::LoadEngine {
                            engine_path: path.clone(),
                        },
                        Some(Duration::from_secs(10)),
                        &cancel,
                    );
                    (path, result)
                }));
            }

            for handle in handles {
                let (path, result) = handle.join().unwrap();
                match result {
                    Ok(FromEngine::Loaded { discovery }) => {
                        assert_eq!(
                            discovery.name, path,
                            "id-correlation failure: request for {path} got wrong payload"
                        );
                    }
                    other => panic!("unexpected result for {path}: {other:?}"),
                }
            }

            let _ = host.shutdown();
        });
    }

    // -----------------------------------------------------------------------
    // Row 15 — Real crash reaps + reports
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: shared `Arc<Mutex<Option<Child>>>` reap in
    // `handle_crash`.  Revert → code/stderr not captured → assertion fails.
    //
    // Child: writes a stderr line, sleeps 0.3s, then kills itself with SIGKILL
    // (so ExitStatus::code() == None).  Never replies on stdout, so any
    // in-flight request will block until the crash.
    #[test]
    fn test_real_crash_reaps_and_reports() {
        watchdog(Duration::from_secs(15), || {
            let host = Arc::new(
                TsEngineHost::start_with_command(
                    {
                        let mut cmd = Command::new("sh");
                        cmd.arg("-c")
                            .arg("echo 'fatal: boom' >&2; sleep 0.3; kill -9 \"$$\"");
                        cmd
                    },
                    make_host_global_config(),
                )
                .expect("start_with_command failed"),
            );

            // Spawn a worker that issues a request — it will never get a reply
            // because the child never writes to stdout.
            let host_req = Arc::clone(&host);
            let worker = std::thread::spawn(move || {
                let cancel = Cancellation::new();
                host_req.request(
                    ToEngine::LoadEngine {
                        engine_path: "/engine.ts".to_string(),
                    },
                    Some(Duration::from_secs(10)),
                    &cancel,
                )
            });

            // The child self-SIGKILLs after ~0.3s; the reader hits EOF and
            // handle_crash broadcasts to all pending slots.
            let result = worker.join().unwrap();
            match result {
                Err(ExecutionError::ProcessCrashed { code, stderr, .. }) => {
                    assert_eq!(
                        code, None,
                        "SIGKILL'd child should have code == None, got {code:?}"
                    );
                    assert!(
                        stderr.contains("fatal: boom"),
                        "stderr should contain 'fatal: boom', got: {stderr:?}"
                    );
                }
                other => panic!("expected ProcessCrashed, got: {other:?}"),
            }
        });
    }

    // -----------------------------------------------------------------------
    // Task 3 — is_alive lifecycle (real child, Unix proc-tier)
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: `is_alive()` — `child.lock().unwrap().is_some()`.
    // Revert (always false) → `assert!(host.is_alive())` after start → RED.
    //
    // Child: `sh -c 'cat >/dev/null'` — stays alive until stdin closes (i.e.
    // until shutdown).  No Deno gate needed: uses only sh/cat.
    #[test]
    fn test_is_alive_lifecycle_real_spawn() {
        watchdog(Duration::from_secs(10), || {
            let host = TsEngineHost::start_with_command(
                {
                    let mut cmd = Command::new("sh");
                    cmd.arg("-c").arg("cat >/dev/null");
                    cmd
                },
                make_host_global_config(),
            )
            .expect("start_with_command failed");

            assert!(
                host.is_alive(),
                "host must report is_alive()==true after start_with_command"
            );

            host.shutdown().expect("shutdown failed");

            assert!(
                !host.is_alive(),
                "host must report is_alive()==false after shutdown()"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Task 3 — spawn_count increments once (Deno-gated)
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: `#[cfg(test)] spawn_count` increment in
    // `ensure_started_inner`.
    // Revert → count stays 0 → `spawn_count() == 1` assertion fails RED.
    //
    // Gate: skip if Deno is absent.  Uses a trivial `cat >/dev/null`-style
    // deno script so we exercise the real spawn path via `start_with_command`.
    #[test]
    fn test_spawn_count_increments_once() {
        if !is_available() {
            return;
        }
        watchdog(Duration::from_secs(30), || {
            use std::io::Write as _;
            use tempfile::NamedTempFile;

            // Minimal Deno script: reads stdin until EOF, then exits.
            let script = r#"
const buf = new Uint8Array(1024);
while (true) {
    const n = await Deno.stdin.read(buf);
    if (n === null) break;
}
"#;
            let mut tmp = NamedTempFile::new().expect("tempfile");
            tmp.write_all(script.as_bytes()).expect("write script");
            let script_path = tmp.path().to_path_buf();

            let mut cmd = Command::new("deno");
            cmd.arg("run").arg("--allow-all").arg(&script_path);
            let host = TsEngineHost::start_with_command(cmd, make_host_global_config())
                .expect("start_with_command failed");

            assert_eq!(
                host.spawn_count(),
                1,
                "after first start, spawn_count must be 1"
            );

            // ensure_started is idempotent — write is already set, init never runs.
            host.ensure_started()
                .expect("second ensure_started must succeed");
            assert_eq!(
                host.spawn_count(),
                1,
                "ensure_started idempotent: spawn_count must stay 1"
            );

            let _ = host.shutdown();
        });
    }
}

// ============================================================================
// Bundle extraction unit test (plain #[cfg(test)], no deno, no unix gate)
// ============================================================================

#[cfg(test)]
mod bundle_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Extraction smoke + idempotency + hash-binding
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: `bundle_content_hash` + content-hash-keyed filename
    // in `extract_bundle_to`.
    //
    // Vacuity guard: assert distinct bytes → distinct filename (not just
    // "file exists"); if both ended up with the same name the content-hash
    // would be non-discriminating.
    #[test]
    fn test_bundle_extraction_idempotent_and_hash_keyed() {
        let dir = TempDir::new().expect("tempdir");
        let bundles_dir = dir.path().join("bundles");

        // --- First extraction ---
        let path1 = extract_bundle_to(&bundles_dir).expect("first extraction");
        assert!(path1.exists(), "extracted file must exist: {path1:?}");

        // Filename must contain the 16-hex hash of the embedded bytes.
        let expected_hash = bundle_content_hash(EMBEDDED_BUNDLE.as_bytes());
        assert_eq!(
            expected_hash.len(),
            16,
            "hash must be 16 hex chars, got: {expected_hash:?}"
        );
        let fname = path1
            .file_name()
            .and_then(|n| n.to_str())
            .expect("filename");
        assert!(
            fname.contains(&expected_hash),
            "filename {fname:?} must contain hash {expected_hash:?}"
        );

        // --- Second extraction is idempotent (same path, no error) ---
        let path2 = extract_bundle_to(&bundles_dir).expect("second extraction");
        assert_eq!(path1, path2, "second extraction must return the same path");

        // --- Distinct bytes → distinct filename (hash-binding vacuity guard) ---
        let alt_bytes = b"different content";
        let alt_hash = bundle_content_hash(alt_bytes);
        let alt_filename = format!("engine-host-deno-{alt_hash}.js");

        // Write a second "bundle" with different bytes directly, then verify
        // the hash would differ from the real one.
        assert_ne!(
            alt_hash, expected_hash,
            "different bytes must produce different hash (vacuity guard)"
        );
        assert_ne!(
            alt_filename, fname,
            "different bytes must produce different filename"
        );
    }

    // -----------------------------------------------------------------------
    // Err-not-cached / Ok-cached / cache-hit skips extractor
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: `cached_extract` — drop the `?` early-return so
    // `extract()` errors are inserted into the cache (e.g. unconditional
    // `*guard = Some(...)` or caching the error path).
    //
    // RED: after call 1 the cache holds a value (error was cached), so call 2
    // returns without running the extractor — `count` stays at 1 and `r2` is
    // Err or the wrong path.
    // GREEN: Err is not cached; call 2 runs the extractor and returns "/x";
    // call 3 hits the cache and skips the extractor entirely.
    #[test]
    fn test_cached_extract_err_not_cached_ok_is_and_hit_skips() {
        let cache: Mutex<Option<PathBuf>> = Mutex::new(None);
        let count = AtomicUsize::new(0);

        // Call 1: extractor fails — result is Err, cache stays empty.
        let r1 = cached_extract(&cache, || {
            count.fetch_add(1, Ordering::Relaxed);
            Err(ExecutionError::other("boom"))
        });
        assert!(r1.is_err(), "call 1 must propagate the error");
        assert!(
            cache.lock().unwrap().is_none(),
            "Err must NOT be cached; cache must remain None after call 1"
        );

        // Call 2: extractor succeeds — result is Ok("/x"), cache populated.
        let r2 = cached_extract(&cache, || {
            count.fetch_add(1, Ordering::Relaxed);
            Ok(PathBuf::from("/x"))
        });
        assert_eq!(r2.unwrap(), PathBuf::from("/x"), "call 2 must return /x");
        assert_eq!(
            count.load(Ordering::Relaxed),
            2,
            "call 2 must have run the extractor (count must be 2)"
        );

        // Call 3: cache hit — extractor never called, returns cached "/x".
        let r3 = cached_extract(&cache, || {
            count.fetch_add(1, Ordering::Relaxed);
            Ok(PathBuf::from("/other"))
        });
        assert_eq!(
            r3.unwrap(),
            PathBuf::from("/x"),
            "call 3 must return cached /x, not /other"
        );
        assert_eq!(
            count.load(Ordering::Relaxed),
            2,
            "call 3 must skip the extractor (count stays 2)"
        );
    }

    /// When QUARTO_CI=1 (set in test-suite.yml), Deno MUST be on PATH — turns the
    /// otherwise-silent `is_available()` skip into a hard CI failure if provisioning
    /// regresses. Locally with no QUARTO_CI set, this is a no-op pass.
    #[test]
    fn deno_available_when_quarto_ci() {
        if std::env::var("QUARTO_CI").as_deref() == Ok("1") {
            assert!(
                is_available(),
                "QUARTO_CI=1 but `deno --version` failed — CI must provision Deno \
                 (test-suite.yml 'Set up Deno' step)"
            );
        }
    }
}
