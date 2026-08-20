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
//! - [`TcpTransport`] / [`TcpReadHalf`] — v1 newline-framed JSON over a
//!   private loopback-TCP socket (the child dials back after a one-time token
//!   handshake).
//! - [`TsEngineHost`] — the multiplexed demux: one reader thread, one pending
//!   map, per-request `sync_channel(1)` slots.
//! - [`MockTransport`] (`#[cfg(test)]`) — id-keyed, blocking `recv()`, for
//!   demux-tier tests that exercise the real reader thread.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::Shutdown;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{self, SyncSender},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
    ///   `Response`. Post-Phase-4, the engine-host protocol rides a private
    ///   loopback-TCP control socket, so a single malformed frame is treated
    ///   as proof the channel is compromised and is fatal immediately — see
    ///   the `Malformed` arm in `reader_loop`.
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
// TcpTransport / TcpReadHalf — loopback-TCP transport (Phase 1a.6)
// ============================================================================
//
// Since the plan1a.6 Phase-3 flip this is the ONLY transport: production
// (`ensure_started`) and the `#[cfg(test)] start_with_command` helper both
// spawn the child with `--control 127.0.0.1:<port>` and complete the one-time
// token handshake in `accept_and_handshake`. The former stdio transport
// (`StdioWriteHalf`/`StdioReadHalf`/`spawn_into`) was deleted at the Phase-4
// cutover. See
// `claude-notes/plans/2026-07-08-plan1a6-off-stdout-loopback-tcp.md`.

/// Shared write half of the loopback-TCP transport.
///
/// Wraps the write side of the accepted `TcpStream`. Internally serialized by
/// a short-held lock (one JSON line, newline-terminated, flushed per frame).
pub struct TcpTransport {
    stream: Mutex<TcpStream>,
}

impl EngineTransport for TcpTransport {
    fn send(&self, frame: &Request) -> Result<(), TransportError> {
        // One JSON line, newline-terminated, flushed per frame.
        let mut guard = self.stream.lock().unwrap();
        let line = serde_json::to_string(frame)?;
        guard.write_all(line.as_bytes())?;
        guard.write_all(b"\n")?;
        guard.flush()?;
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TransportError> {
        let mut guard = self.stream.lock().unwrap();
        // Send Shutdown frame first (best-effort) — throwaway id, one
        // newline-framed JSON line.
        let shutdown_frame = Request {
            id: u64::MAX, // throwaway id — no slot registered for Shutdown
            msg: ToEngine::Shutdown,
        };
        if let Ok(line) = serde_json::to_string(&shutdown_frame) {
            let _ = guard.write_all(line.as_bytes());
            let _ = guard.write_all(b"\n");
            let _ = guard.flush();
        }
        // Half-close the write side — the TCP analogue of dropping stdin:
        // the peer's read side sees EOF, but our own read half (a separate
        // clone of the accepted stream) is untouched.
        let _ = guard.shutdown(Shutdown::Write);
        Ok(())
    }
}

/// Owned read half of the loopback-TCP transport.
///
/// Holds the handshake `BufReader<TcpStream>` — `accept_and_handshake`
/// constructs this from the accepted stream's `try_clone()`, the same stream
/// `TcpTransport` writes to.
pub struct TcpReadHalf {
    read: BufReader<TcpStream>,
}

impl EngineReadHalf for TcpReadHalf {
    fn recv(&mut self) -> Result<Response, RecvError> {
        // EOF/read-0 AND an empty line both mean "channel closed"; a non-empty
        // line that fails to parse as `Response` is `Malformed` (surfaced to
        // `reader_loop`, which since Phase 4 treats a malformed control-socket
        // frame as fatal).
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

/// Constant-time byte-slice equality (XOR-fold over every byte).
///
/// Hygiene, not load-bearing: the listener accepts exactly once, then closes
/// (see H-COMMIT / the design note in
/// `claude-notes/plans/2026-07-08-plan1a6-off-stdout-loopback-tcp.md`), so
/// there is no repeated-guess timing oracle to defend against in practice.
/// Unequal-length inputs short-circuit to `false`; equal-length inputs always
/// walk every byte regardless of where (or whether) they differ.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

/// How long between `accept()` polls while waiting for the child to dial
/// back over loopback TCP (H-ACCEPT).
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Accept the one expected connection on `listener`, perform the one-time
/// token handshake, and return the split transport halves.
///
/// `child` is the shared child-process slot (so a handshake failure can
/// correlate with a dead child); `token` is the one-time handshake secret;
/// `deadline` bounds how long `accept()` + the handshake read may take.
///
/// `listener` is taken **by value** and is never explicitly closed: it drops
/// (closing the OS socket) when this function returns by any path, which is
/// what makes the "at most one dial ever succeeds" property (seam #2c)
/// structural rather than something this function has to enforce itself.
fn accept_and_handshake(
    listener: TcpListener,
    child: &Arc<Mutex<Option<Child>>>,
    token: &str,
    deadline: Duration,
) -> Result<(Arc<TcpTransport>, TcpReadHalf), ExecutionError> {
    // Captured before the poll loop purely for the connected-marker log at
    // commit time; the listener itself is polled by reference below and
    // drops structurally when this function returns.
    let port = listener.local_addr().map_or(0, |addr| addr.port());

    listener.set_nonblocking(true).map_err(|e| {
        ExecutionError::other(format!(
            "failed to set loopback-TCP listener nonblocking: {e}"
        ))
    })?;

    let start = Instant::now();

    // H-ACCEPT: poll accept() against child liveness AND the deadline. A
    // child that has already exited fails fast (no reason to wait out the
    // deadline for a dial that will never come); a still-alive child that
    // simply hasn't dialed yet is bounded by `deadline`.
    let stream = loop {
        match listener.accept() {
            Ok((stream, _peer)) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let child_exited = child
                    .lock()
                    .unwrap()
                    .as_mut()
                    .is_some_and(|c| matches!(c.try_wait(), Ok(Some(_))));
                if child_exited {
                    return Err(ExecutionError::other(
                        "engine-host child exited before dialing back over loopback TCP",
                    ));
                }
                if start.elapsed() > deadline {
                    if let Some(mut c) = child.lock().unwrap().take() {
                        let _ = c.kill();
                    }
                    return Err(ExecutionError::other(format!(
                        "timed out after {deadline:?} waiting for engine-host to connect over loopback TCP"
                    )));
                }
                std::thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(e) => {
                return Err(ExecutionError::other(format!(
                    "failed to accept loopback-TCP connection: {e}"
                )));
            }
        }
    };

    // The listener was switched to nonblocking for the poll loop above; on
    // Windows the accepted socket inherits that flag (Linux/macOS do not),
    // so restore blocking mode before the handshake read below.
    stream.set_nonblocking(false).map_err(|e| {
        ExecutionError::other(format!(
            "failed to clear nonblocking mode on accepted stream: {e}"
        ))
    })?;
    stream
        .set_nodelay(true)
        .map_err(|e| ExecutionError::other(format!("failed to set TCP_NODELAY: {e}")))?;

    // H-TOKEN / H-READER: exactly one `try_clone()`. The `BufReader` built
    // here over the clone is THE handshake reader — it becomes `TcpReadHalf`
    // unchanged at commit, so any bytes coalesced past the token's `\n` in
    // the same segment are preserved rather than dropped by a fresh reader.
    let read_clone = stream
        .try_clone()
        .map_err(|e| ExecutionError::other(format!("failed to clone accepted stream: {e}")))?;
    let mut reader = BufReader::new(read_clone);

    let mut token_line = String::new();
    // Bounded read: `Take` caps the total bytes `read_line` may consume, so a
    // connector that never sends a newline within `MAX_TOKEN_LINE` bytes
    // cannot block this read indefinitely — `Take` reports EOF (Ok(0)) once
    // its limit is exhausted, which `read_line` treats as "no more data".
    let mut bounded = (&mut reader).take(MAX_TOKEN_LINE as u64);
    let read_ok = bounded.read_line(&mut token_line).is_ok();

    if !read_ok || !token_line.ends_with('\n') {
        if let Some(mut c) = child.lock().unwrap().take() {
            let _ = c.kill();
        }
        return Err(ExecutionError::other(
            "engine-host handshake token line was missing, truncated, or exceeded the length cap",
        ));
    }

    let received = token_line
        .trim_end_matches('\n')
        .trim_end_matches('\r')
        .as_bytes();
    if !ct_eq(received, token.as_bytes()) {
        if let Some(mut c) = child.lock().unwrap().take() {
            let _ = c.kill();
        }
        return Err(ExecutionError::other(
            "engine-host handshake token mismatch",
        ));
    }

    // H-COMMIT: the original `stream` becomes the write half; the handshake
    // `BufReader` (already holding any bytes past the token's `\n`) becomes
    // the read half, unchanged.
    tracing::info!(target: "engine_host", port, "engine-host connected over loopback TCP");

    let transport = TcpTransport {
        stream: Mutex::new(stream),
    };
    let read_half = TcpReadHalf { read: reader };

    Ok((Arc::new(transport), read_half))
}

/// Spawn a child from `cmd` (already configured with `--control <port>` or
/// equivalent by the caller) and complete the loopback-TCP handshake against
/// `listener`.
///
/// Stores the `Child` in `child_slot` (shared with the read half); returns the
/// write half, read half, and the two drain-thread handles (stderr, stdout).
///
/// Order (load-bearing):
/// 1. Pipe all three stdio streams and spawn; store the `Child` in
///    `child_slot`.
/// 2. **H-SPAWN(a):** write `<token>\n` to the child's stdin, flush, then
///    drop the `ChildStdin` — closing it, so the child sees EOF right after
///    the token (stdin carries nothing else).
/// 3. **H-DRAIN:** spawn both drain threads (`stderr_loop` fills the crash
///    ring; `stdout_loop` forwards to tracing) *before* the accept/handshake,
///    so a child that dies mid-handshake still has its output fully drained.
/// 4. Run [`accept_and_handshake`].
/// 5. On success, hand back both drain-thread handles alongside the split
///    transport — still running, owned by the caller from here on.
/// 6. On failure, this function owns cleanup: kill the child if it's still
///    in `child_slot`, join both drain threads (guaranteeing a dead child's
///    stderr has fully landed in `recent_stderr`), then return an
///    [`ExecutionError`] enriched with a snapshot of that ring.
pub fn spawn_into_tcp(
    mut cmd: Command,
    child_slot: Arc<Mutex<Option<Child>>>,
    listener: TcpListener,
    token: &str,
    deadline: Duration,
    recent_stderr: Arc<Mutex<VecDeque<String>>>,
) -> Result<
    (
        Arc<TcpTransport>,
        TcpReadHalf,
        JoinHandle<()>,
        JoinHandle<()>,
    ),
    ExecutionError,
> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| ExecutionError::other(format!("failed to spawn engine host: {e}")))?;

    let mut stdin = child
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

    *child_slot.lock().unwrap() = Some(child);

    // H-SPAWN(a): write the token, flush, then drop stdin — closing it so
    // the child sees EOF immediately after the token line.
    if let Err(e) = stdin
        .write_all(format!("{token}\n").as_bytes())
        .and_then(|_| stdin.flush())
    {
        if let Some(mut c) = child_slot.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        return Err(ExecutionError::other(format!(
            "failed to write handshake token to engine-host stdin: {e}"
        )));
    }
    drop(stdin);

    // H-DRAIN: both drain threads start now, before the accept — so a child
    // that dies during the handshake still has stderr/stdout fully drained.
    let stderr_handle = {
        let recent_stderr = Arc::clone(&recent_stderr);
        std::thread::spawn(move || stderr_loop(BufReader::new(stderr), recent_stderr))
    };
    let stdout_handle = std::thread::spawn(move || stdout_loop(BufReader::new(stdout)));

    match accept_and_handshake(listener, &child_slot, token, deadline) {
        Ok((transport, read_half)) => Ok((transport, read_half, stderr_handle, stdout_handle)),
        Err(e) => {
            // Own the cleanup: kill the child if accept_and_handshake left it
            // alive (some of its error paths already killed it; take()
            // no-ops harmlessly in that case).
            if let Some(mut c) = child_slot.lock().unwrap().take() {
                let _ = c.kill();
                let _ = c.wait();
            }

            // Join both drains so the dead child's stderr (read to EOF once
            // its pipe closed) has fully landed in `recent_stderr` before the
            // snapshot below.
            let _ = stderr_handle.join();
            let _ = stdout_handle.join();

            let snapshot = recent_stderr
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");

            Err(ExecutionError::other(format!(
                "{e}; recent engine-host stderr:\n{snapshot}"
            )))
        }
    }
}

/// The loopback-TCP dial-back preamble that every `deno_dialback_child` test
/// script begins with: parse `--control 127.0.0.1:<port>` off argv, read the
/// one-time token from the first line of stdin, `Deno.connect`, and present
/// the token as the socket pre-line — the tiny hand-rolled equivalent of the
/// real bundle's `connectControl` (`control-transport.ts`). It leaves three
/// bindings in scope for the per-test body: the connected socket `conn`, a
/// `TextDecoder dec`, and a `TextEncoder enc`.
#[cfg(test)]
const DIALBACK_PREAMBLE: &str = r#"
const args = Deno.args;
let control = null;
for (let i = 0; i < args.length; i++) if (args[i] === "--control") control = args[i + 1];
if (!control) { console.error("dialback: no --control arg"); Deno.exit(2); }
const dec = new TextDecoder();
const enc = new TextEncoder();
const sbuf = new Uint8Array(4096);
let acc = "";
let token = null;
while (token === null) {
  const n = await Deno.stdin.read(sbuf);
  if (n === null) break;
  acc += dec.decode(sbuf.subarray(0, n));
  const idx = acc.indexOf("\n");
  if (idx !== -1) token = acc.slice(0, idx);
}
if (token === null) { console.error("dialback: no token on stdin"); Deno.exit(3); }
const [ctlHost, ctlPort] = control.split(":");
const conn = await Deno.connect({ hostname: ctlHost, port: Number(ctlPort) });
await conn.write(enc.encode(token + "\n"));
"#;

/// A [`deno_dialback_child`] body that discards everything the host writes and
/// exits 0 when the socket read side closes (the host's `shutdown()` half-close,
/// or `Drop`) — the loopback-TCP equivalent of the old `sh -c 'cat >/dev/null'`
/// liveness child.
#[cfg(test)]
pub(crate) const DIALBACK_READ_UNTIL_EOF: &str = r#"
const rbuf = new Uint8Array(4096);
while (true) { const n = await conn.read(rbuf); if (n === null) break; }
Deno.exit(0);
"#;

/// Test-only: build a `deno run` child that performs the loopback-TCP dial-back
/// handshake ([`DIALBACK_PREAMBLE`]) and then runs `body` (which may use the
/// `conn`/`dec`/`enc` bindings the preamble leaves in scope).
///
/// Deno reads the script file at process startup, so the returned
/// `NamedTempFile` must be kept alive until `start_with_command` returns; the
/// caller binds it (`let (cmd, _script) = ...`) for the duration of the test.
/// `start_with_command` appends the `--control 127.0.0.1:<port>` argument, so
/// callers must NOT add it themselves.
#[cfg(test)]
pub(crate) fn deno_dialback_child(body: &str) -> (Command, tempfile::NamedTempFile) {
    use std::io::Write as _;
    let script = format!("{DIALBACK_PREAMBLE}\n{body}\n");
    let mut tmp = tempfile::NamedTempFile::new().expect("create dialback tempfile");
    tmp.write_all(script.as_bytes())
        .expect("write dialback script");
    tmp.flush().expect("flush dialback script");
    let mut cmd = Command::new("deno");
    cmd.arg("run").arg("--allow-all").arg(tmp.path());
    (cmd, tmp)
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
    /// The stdout drain thread (loopback-TCP transport — `StartedDrains::Tcp`).
    /// `None` for mock-transport tests (`StartedDrains::None`), which have no
    /// real child and thus no stdout to drain.
    stdout_reader: Mutex<Option<JoinHandle<()>>>,
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

/// Maximum length (bytes) of the one-time handshake token line read from the
/// accepted TCP connection before the loopback-TCP transport gives up and
/// treats the connection as malformed. Bounds the handshake read so a
/// misbehaving (or malicious-on-loopback) connector can't make the accept
/// path buffer unboundedly while waiting for a newline.
const MAX_TOKEN_LINE: usize = 256;

/// What (if anything) `ensure_started_inner`'s `init` spawned to drain the
/// child's out-of-band output channels, so the caller knows which thread(s)
/// to start and which join-handle field(s) to populate.
///
/// - `Tcp { .. }` — the loopback-TCP transport (the ONLY live transport since
///   the plan1a.6 Phase-3 flip): neither stdout nor stderr is the demux
///   channel (that's the accepted socket), so BOTH need drain threads and the
///   caller stores both handles. Constructed by `ensure_started` (production)
///   and `#[cfg(test)] start_with_command` (real-child tests).
/// - `None` — no drain thread needed (mock-transport tests, no real child).
///   `#[cfg(test)]`-gated: it is constructed only under `cfg(test)`, so in a
///   plain (non-test / WASM) lib build the enum has just the live `Tcp`
///   variant and needs no `#[allow(dead_code)]`.
enum StartedDrains {
    #[cfg(test)]
    None,
    Tcp {
        stderr: JoinHandle<()>,
        stdout: JoinHandle<()>,
    },
}

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
            stdout_reader: Mutex::new(None),
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
    /// Drives the exact production loopback-TCP path (bind an ephemeral control
    /// listener, mint a one-time token, append `--control 127.0.0.1:<port>` to
    /// `cmd`, spawn via [`spawn_into_tcp`], and complete the token handshake),
    /// then hands the split halves to `ensure_started_inner` — so the reader
    /// thread, both drain threads, and all teardown paths are exercised exactly
    /// as in production. The caller-supplied `cmd` must be a child that dials
    /// back over the control socket (see the test-only `deno_dialback_child`
    /// helper); a child that never connects makes this time out at the accept
    /// deadline, exactly as production would.
    #[cfg(test)]
    pub fn start_with_command(
        cmd: Command,
        global: HostGlobalConfig,
    ) -> Result<Self, ExecutionError> {
        let host = Self::new(global);
        let child_arc = Arc::clone(&host.child);
        let recent_stderr = Arc::clone(&host.recent_stderr);
        host.ensure_started_inner(move || {
            let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| {
                ExecutionError::other(format!("failed to bind loopback control listener: {e}"))
            })?;
            let port = listener
                .local_addr()
                .map_err(|e| {
                    ExecutionError::other(format!("failed to read control listener addr: {e}"))
                })?
                .port();
            let token = uuid::Uuid::new_v4().to_string();

            let mut cmd = cmd;
            cmd.arg("--control").arg(format!("127.0.0.1:{port}"));

            let (transport, read_half, stderr, stdout) = spawn_into_tcp(
                cmd,
                child_arc,
                listener,
                &token,
                Duration::from_secs(10),
                recent_stderr,
            )?;
            Ok((
                transport as Arc<dyn EngineTransport>,
                Box::new(read_half) as Box<dyn EngineReadHalf>,
                StartedDrains::Tcp { stderr, stdout },
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

            // plan1a.6: bind an ephemeral loopback control listener BEFORE spawn. std's
            // TcpListener::bind calls listen(), so the kernel backlogs the child's dial-back
            // from bind onward — no race window between bind and spawn.
            let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| {
                ExecutionError::other(format!("failed to bind loopback control listener: {e}"))
            })?;
            let port = listener
                .local_addr()
                .map_err(|e| {
                    ExecutionError::other(format!("failed to read control listener addr: {e}"))
                })?
                .port();

            // One-time handshake token (122-bit uuid). Delivered to the child on STDIN by
            // spawn_into_tcp (NOT argv — argv would leak the secret via ps/cmdline); the
            // child presents it as the socket pre-line that accept_and_handshake validates.
            let token = uuid::Uuid::new_v4().to_string();

            let mut cmd = Command::new("deno");
            // `--allow-all` is the ACCEPTED v1 security posture (decided 2026-07-01), not an
            // oversight. Extension bundles are third-party code running at full Deno privilege;
            // the v1 trust model is "the user installed the extension deliberately." The eventual
            // real boundary is Phase 1.6 (loopback-TCP transport + one-time token auth), not a
            // Deno permission set — so any future narrowing to `--allow-read/write/net/run` here
            // is a deliberate, separately-reviewed change, not a drive-by tightening.
            cmd.arg("run")
                .arg("--allow-all")
                .arg(bundle_path)
                .arg("--control")
                .arg(format!("127.0.0.1:{port}"));

            let (transport, read_half, stderr, stdout) = spawn_into_tcp(
                cmd,
                Arc::clone(&self.child),
                listener,
                &token,
                Duration::from_secs(10),
                Arc::clone(&self.recent_stderr),
            )?;
            Ok((
                transport as Arc<dyn EngineTransport>,
                Box::new(read_half) as Box<dyn EngineReadHalf>,
                StartedDrains::Tcp { stderr, stdout },
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
                StartedDrains,
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

        let (write, read, drains) = init()?;

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

        // Spawn/store drain thread(s) per what `init` actually started.
        match drains {
            // Mock transport (tests): no drain thread to spawn.
            #[cfg(test)]
            StartedDrains::None => {}
            // Loopback-TCP transport: `init` already spawned both drain
            // threads (neither stdout nor stderr is the demux channel there —
            // that's the accepted socket) — just store the handles.
            StartedDrains::Tcp { stderr, stdout } => {
                *self.stderr_reader.lock().unwrap() = Some(stderr);
                *self.stdout_reader.lock().unwrap() = Some(stdout);
            }
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
        if let Some(handle) = self.stdout_reader.lock().unwrap().take() {
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
        if let Some(handle) = self.stdout_reader.get_mut().unwrap().take() {
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
    loop {
        match read.recv() {
            Ok(Response { id, msg }) => {
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
                // Post-Phase-4 (plan 2026-07-08-plan1a6-off-stdout-loopback-tcp.md,
                // H-MALFORMED): the engine-host protocol rides a private
                // loopback-TCP control socket. Nothing benign can write to
                // that socket, so a malformed (non-JSON) frame on it is
                // genuine evidence the channel is compromised — not a stray
                // console.log leaking onto a shared stdout fd (that footgun
                // is gone with stdout as the channel). A single malformed
                // frame is therefore fatal immediately: no tolerate-then-
                // escalate window (Phases 1-3 kept a bounded log-and-skip
                // leniency here — `MAX_CONSECUTIVE_MALFORMED_LINES` — which
                // Phase 4 deletes).
                //
                // Set shutting_down FIRST so the kill below doesn't re-enter
                // the crash path (finding #7 — one terminal error per exit).
                shutting_down.store(true, Ordering::Relaxed);

                let excerpt: String = line.chars().take(200).collect();
                let msg = format!(
                    "engine-host protocol error: malformed frame on the control \
                     socket — channel considered compromised, terminating engine \
                     host (line: {excerpt:?})"
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

    // Reap the exit code (single-shot via take). Kill before wait: the EOF
    // that got us here means the demux channel closed, but that is NOT
    // proof the process itself has exited (e.g. the loopback-TCP transport's
    // socket can close while the child stays alive) — a bare `wait()` would
    // then block indefinitely. Kill-then-wait matches the style already used
    // at `reset_after_crash`, `Drop`, and the malformed-channel escalation
    // above; harmless when the child already exited (killing a zombie is a
    // no-op).
    let code = child
        .lock()
        .unwrap()
        .take()
        .and_then(|mut c| {
            let _ = c.kill();
            c.wait().ok()
        })
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

/// Drain the child's stdout, logging each line via `tracing::info!`.
///
/// Only relevant on the loopback-TCP transport, where stdout is no longer the
/// demux channel (the accepted socket is) — the child's stdout is just
/// ordinary process chatter, same status as stderr on the stdio transport
/// minus the crash-ring bookkeeping: no demux, no `recent_stderr` ring, just
/// forwarding to tracing. Mirrors `stderr_loop`'s line-reading shape.
///
/// Live production since the plan1a.6 Phase-3 flip: its caller `spawn_into_tcp`
/// is now on the real `ensure_started` path (loopback-TCP transport).
fn stdout_loop(reader: impl BufRead) {
    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };
        info!(target: "engine_host", "{}", line);
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
    // Seam #6b — malformed control-socket frame is fatal (H-MALFORMED)
    // -----------------------------------------------------------------------
    //
    // Plan 2026-07-08-plan1a6-off-stdout-loopback-tcp.md, Phase 4 (the
    // cutover). Post-Phase-3 the engine-host protocol rides a private
    // loopback-TCP control socket; nothing benign can write to it, so a
    // single malformed (non-JSON) frame is genuine evidence the channel is
    // compromised. This replaces the Phase 1-3 `MAX_CONSECUTIVE_MALFORMED_LINES`
    // log-and-skip leniency (a relic of stdout-as-channel, where a stray
    // `console.log` could leak onto the shared fd): the FIRST malformed
    // frame must now be fatal, not the sixth.
    //
    // Named revert hunk (H-MALFORMED): re-add the bounded log-and-skip
    // `continue` for the first malformed line in the `RecvError::Malformed`
    // arm of `reader_loop` → the single injected line is skipped instead of
    // escalated → the in-flight request never resolves → the `watchdog`
    // fires → RED.
    #[test]
    fn test_malformed_frame_is_fatal() {
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

            // Exactly ONE malformed line — the strict Phase-4 policy must
            // treat this as fatal immediately, with no tolerate window.
            mock.signal_malformed_many(&["console.log('one stray line')"]);

            let result = request_handle.join().unwrap();

            // Must be Other(_) (channel-compromised broadcast) — NOT
            // ProcessCrashed, NOT Ok.
            assert!(
                matches!(result, Err(ExecutionError::Other(_))),
                "expected Other (channel-compromised broadcast), got: {result:?}"
            );

            assert!(
                host.is_shutting_down(),
                "a single malformed frame on the control socket must mark shutting_down"
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
                        Ok((write, read, StartedDrains::None))
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
            host.ensure_started_inner(|| Ok((w1, r1, StartedDrains::None)))
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
            host.ensure_started_inner(|| Ok((w2, r2, StartedDrains::None)))
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
                    Ok((write, read, StartedDrains::None))
                })
                .expect("first ensure_started_inner");
                // Idempotent: write is already committed → init never runs →
                // no second spawn event.
                host.ensure_started_inner(|| {
                    let (write, read) = MockTransport::pair();
                    Ok((write, read, StartedDrains::None))
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
                            Ok((write, read, StartedDrains::None))
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

    // -----------------------------------------------------------------------
    // TcpTransport / TcpReadHalf — H-FRAME round trip (Phase 1a.6 seam #1)
    // -----------------------------------------------------------------------
    //
    // The peer (a plain `TcpListener`/`TcpStream` pair, NOT the unit under
    // test) is the mock boundary: it accepts the connection, reads one
    // framed request line, and echoes back one framed response line under
    // the same `id`. `TcpTransport`/`TcpReadHalf` are constructed directly
    // over a `try_clone()`'d connected socket, exactly as `spawn_into_tcp`
    // will do in a later task.
    //
    // Named revert hunk: the trailing-newline write in `TcpTransport::send`.
    // Reverting it collapses the request line into the peer's next
    // `read_line` call indefinitely (no frame terminator ever arrives), so
    // the peer thread hangs in `accept`/`read_line` and `recv()` never sees
    // a reply — caught here as a `watchdog` timeout, not a silent hang.
    #[test]
    fn test_tcp_transport_round_trip() {
        watchdog(Duration::from_secs(15), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let addr = listener.local_addr().expect("local_addr");

            let peer = std::thread::spawn(move || {
                let (stream, _) = listener.accept().expect("peer accept");
                let mut reader = BufReader::new(stream.try_clone().expect("peer try_clone"));
                let mut writer = stream;

                let mut line = String::new();
                reader.read_line(&mut line).expect("peer read_line");
                let request: Request =
                    serde_json::from_str(line.trim_end_matches('\n').trim_end_matches('\r'))
                        .expect("peer parse request");

                let response = Response {
                    id: request.id,
                    msg: make_loaded_response("engine-tcp"),
                };
                let out = serde_json::to_string(&response).expect("peer serialize response");
                writer
                    .write_all(out.as_bytes())
                    .expect("peer write response");
                writer.write_all(b"\n").expect("peer write newline");
                writer.flush().expect("peer flush");
            });

            let stream = TcpStream::connect(addr).expect("connect to peer");
            let write_half = TcpTransport {
                stream: Mutex::new(stream.try_clone().expect("try_clone write half")),
            };
            let mut read_half = TcpReadHalf {
                read: BufReader::new(stream.try_clone().expect("try_clone read half")),
            };

            let request = Request {
                id: 42,
                msg: make_load_engine_msg(),
            };
            write_half.send(&request).expect("TcpTransport::send");

            let response = read_half.recv().expect("TcpReadHalf::recv");
            assert_eq!(
                response,
                Response {
                    id: 42,
                    msg: make_loaded_response("engine-tcp"),
                },
                "round-tripped response must match the peer's echoed frame exactly"
            );

            peer.join().expect("peer thread panicked");
        });
    }

    // -----------------------------------------------------------------------
    // accept_and_handshake — H-ACCEPT / H-TOKEN / H-READER / H-COMMIT
    // (Phase 1a.6 seams #2(a,b,d,e), #2c, #3)
    // -----------------------------------------------------------------------
    //
    // These tests call `accept_and_handshake` directly against a real
    // listener + a real (short-lived, non-bundle) child process — the child
    // only needs to exist/stay alive, never the real deno engine-host.

    /// Spawn a child that stays alive for roughly `secs` seconds and does
    /// nothing else — a stand-in for the real deno child in these
    /// handshake-only tests (portable per the cross-platform rule; unix uses
    /// `sleep`, everything else falls back to `powershell Start-Sleep`).
    #[cfg(unix)]
    fn spawn_long_lived_child(secs: u64) -> Child {
        Command::new("sleep")
            .arg(secs.to_string())
            .spawn()
            .expect("spawn sleep child")
    }

    #[cfg(not(unix))]
    fn spawn_long_lived_child(secs: u64) -> Child {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("Start-Sleep -Seconds {secs}"),
            ])
            .spawn()
            .expect("spawn Start-Sleep child")
    }

    // #2a (POSITIVE CONTROL — no revert hunk; #2b/#2d/#2e are the reverts
    // that redden relative to this green).
    #[test]
    fn test_handshake_accepts_correct_token() {
        watchdog(Duration::from_secs(2), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let addr = listener.local_addr().expect("local_addr");
            let token = "correct-token".to_string();
            let child = Arc::new(Mutex::new(Some(spawn_long_lived_child(5))));

            let dial_token = token.clone();
            let dialer = std::thread::spawn(move || {
                let mut stream = TcpStream::connect(addr).expect("dial listener");
                stream
                    .write_all(format!("{dial_token}\n").as_bytes())
                    .expect("write token");
            });

            let result = accept_and_handshake(listener, &child, &token, Duration::from_secs(1));
            dialer.join().expect("dialer thread panicked");

            assert!(
                result.is_ok(),
                "correct token must be accepted: {:?}",
                result.err()
            );

            if let Some(mut c) = child.lock().unwrap().take() {
                let _ = c.kill();
            }
        });
    }

    // Named revert hunk: the `ct_eq` compare in `accept_and_handshake`'s
    // H-TOKEN step. Neutralizing it to "always equal" makes a wrong token
    // get accepted (Ok) instead of rejected → assertion fails RED.
    #[test]
    fn test_handshake_rejects_wrong_token() {
        watchdog(Duration::from_secs(2), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let addr = listener.local_addr().expect("local_addr");
            let token = "correct-token".to_string();
            let child = Arc::new(Mutex::new(Some(spawn_long_lived_child(5))));

            let dialer = std::thread::spawn(move || {
                let mut stream = TcpStream::connect(addr).expect("dial listener");
                stream
                    .write_all(b"wrong-token\n")
                    .expect("write wrong token");
            });

            let result = accept_and_handshake(listener, &child, &token, Duration::from_secs(1));
            dialer.join().expect("dialer thread panicked");

            assert!(result.is_err(), "a wrong token must be rejected");

            if let Some(mut c) = child.lock().unwrap().take() {
                let _ = c.kill();
            }
        });
    }

    // Named revert hunk: the deadline-expiry branch (`start.elapsed() >
    // deadline`) in the H-ACCEPT poll loop. Removing it leaves only the
    // `try_wait`-exited check, so a live child that never dials loops
    // forever on `WouldBlock` → watchdog RED (not a normal test failure).
    #[test]
    fn test_handshake_deadline_with_live_child() {
        watchdog(Duration::from_secs(2), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            // Child stays alive well past the injected deadline — no dialer
            // ever connects, so only the deadline path (not the try_wait
            // fast-exit path) can end this.
            let child = Arc::new(Mutex::new(Some(spawn_long_lived_child(30))));
            let token = "deadline-token".to_string();

            let start = Instant::now();
            let result = accept_and_handshake(listener, &child, &token, Duration::from_millis(400));
            let elapsed = start.elapsed();

            assert!(
                result.is_err(),
                "no dialer + a live child must time out via the deadline path"
            );
            assert!(
                elapsed < Duration::from_secs(2),
                "deadline path must return well under the watchdog bound, took {elapsed:?}"
            );

            if let Some(mut c) = child.lock().unwrap().take() {
                let _ = c.kill();
            }
        });
    }

    // Named revert hunk: the `MAX_TOKEN_LINE` bound on the handshake read
    // (the `.take(MAX_TOKEN_LINE as u64)` wrapper). Removing it makes the
    // token read a plain unbounded `read_line`, which blocks forever waiting
    // for a newline that never comes → watchdog RED.
    #[test]
    fn test_handshake_rejects_overlong_token() {
        watchdog(Duration::from_secs(2), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let addr = listener.local_addr().expect("local_addr");
            let token = "overlong-token".to_string();
            let child = Arc::new(Mutex::new(Some(spawn_long_lived_child(5))));

            let mut dialer_stream = TcpStream::connect(addr).expect("dial listener");
            // More than MAX_TOKEN_LINE bytes, deliberately no `\n`.
            let payload = vec![b'x'; MAX_TOKEN_LINE + 64];
            dialer_stream
                .write_all(&payload)
                .expect("write overlong no-newline payload");
            // Hold the connection open (do NOT close it) rather than
            // spawning a thread to sleep on it: in the reverted
            // (unbounded-read) state, a closed connection would make
            // `read_line` see EOF and return promptly, hiding the very hang
            // this test exists to catch. Leaking the `TcpStream` keeps the
            // socket open without a lingering background thread (which would
            // otherwise trip nextest's leak detector even on the passing,
            // non-reverted run).
            std::mem::forget(dialer_stream);

            let result = accept_and_handshake(listener, &child, &token, Duration::from_secs(1));

            assert!(
                result.is_err(),
                "an overlong token line with no newline within the cap must be rejected"
            );

            if let Some(mut c) = child.lock().unwrap().take() {
                let _ = c.kill();
            }
        });
    }

    // #2c — INVARIANT, no paired revert hunk (the listener-close is
    // structural: moved into `accept_and_handshake` by value, dropped on
    // return). Reddens only if a future refactor lets the listener outlive
    // the handshake.
    #[test]
    fn test_single_dial_invariant() {
        watchdog(Duration::from_secs(2), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let addr = listener.local_addr().expect("local_addr");
            let token = "single-dial-token".to_string();
            let child = Arc::new(Mutex::new(Some(spawn_long_lived_child(5))));

            let dial_token = token.clone();
            let dialer = std::thread::spawn(move || {
                let mut stream = TcpStream::connect(addr).expect("dial listener");
                stream
                    .write_all(format!("{dial_token}\n").as_bytes())
                    .expect("write token");
            });

            let result = accept_and_handshake(listener, &child, &token, Duration::from_secs(1));
            dialer.join().expect("dialer thread panicked");
            assert!(
                result.is_ok(),
                "setup handshake must succeed: {:?}",
                result.err()
            );

            // The listener was moved into accept_and_handshake by value and
            // dropped (fd closed) before it returned — so a second dial to the
            // same address must be *refused at connect()* (ECONNREFUSED): with
            // no LISTEN socket on the port, the SYN gets an RST. This is the
            // plan's explicit assertion ("must fail — not merely 'not be
            // accepted'"): asserting only that a read/write later fails would
            // be VACUOUS, because an open-but-unaccepted listener also lets
            // connect() succeed while a probe read merely times out. Verified
            // by leaking the listener past the return — that reddens this test
            // only with the strict `is_err()` check, not the round-trip check.
            let second = TcpStream::connect(addr);
            assert!(
                second.is_err(),
                "a second dial after a successful handshake must be REFUSED at connect() \
                 (the listener must be closed structurally); got Ok, so the listener outlived \
                 the handshake"
            );

            if let Some(mut c) = child.lock().unwrap().take() {
                let _ = c.kill();
            }
        });
    }

    // #3 — Named revert hunk: building `TcpReadHalf` from a FRESH
    // `BufReader::new(stream.try_clone()?)` instead of the handshake reader.
    // The coalesced frame bytes were already pulled into the handshake
    // reader's buffer, so a fresh reader loses them → `recv()` blocks
    // (watchdog RED) or errors.
    #[test]
    fn test_reader_handoff_coalesced_frame() {
        watchdog(Duration::from_secs(2), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let addr = listener.local_addr().expect("local_addr");
            let token = "coalesced-token".to_string();
            let child = Arc::new(Mutex::new(Some(spawn_long_lived_child(5))));

            let response = Response {
                id: 7,
                msg: make_loaded_response("engine-coalesced"),
            };
            let frame_line = serde_json::to_string(&response).expect("serialize response");

            let dial_token = token.clone();
            let dialer = std::thread::spawn(move || {
                let mut stream = TcpStream::connect(addr).expect("dial listener");
                // Coalesce token + frame into ONE write_all so both are very
                // likely to land in the same TCP segment — the hazard
                // H-READER exists to survive.
                let payload = format!("{dial_token}\n{frame_line}\n");
                stream
                    .write_all(payload.as_bytes())
                    .expect("write coalesced token+frame payload");
            });

            let (_write_half, mut read_half) =
                accept_and_handshake(listener, &child, &token, Duration::from_secs(1))
                    .expect("handshake must succeed");
            dialer.join().expect("dialer thread panicked");

            let received = read_half.recv().expect("recv coalesced frame");
            assert_eq!(
                received, response,
                "the frame coalesced with the token in one segment must not be dropped"
            );

            if let Some(mut c) = child.lock().unwrap().take() {
                let _ = c.kill();
            }
        });
    }

    // Standing property test — like seam #10, no named revert hunk. Migrated
    // from the deleted `ts_process_framing_probe.rs` (`pc_c_a`) at the plan1a.6
    // Phase-4 cutover: the old stdout-reader path (`StdioReadHalf` over a deno
    // child) is gone, so the property is now pinned over the live TCP transport.
    //
    // Property: a >1 MB single-line frame round-trips through `TcpReadHalf::recv`
    // intact. `read_line` has no size cap — it grows the `String` over the
    // BufReader's (8 KB) internal buffer until `\n`/EOF — so a multi-MB frame
    // terminated by a single `\n` must survive without truncation or mis-split.
    // Distinct from seam #10 (deadlock-freedom under a *small* payload against
    // shrunk socket buffers); different size, different assertion.
    #[test]
    fn test_large_single_line_frame_parses_over_tcp() {
        watchdog(Duration::from_secs(10), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let addr = listener.local_addr().expect("local_addr");
            let token = "large-frame-token".to_string();
            let child = Arc::new(Mutex::new(Some(spawn_long_lived_child(5))));

            // ~2 MB payload inside a valid FromEngine::Error frame, one newline.
            let big = "x".repeat(2_000_000);
            let response = Response {
                id: 7,
                msg: FromEngine::Error {
                    message: big,
                    stack: None,
                },
            };
            let frame_line = serde_json::to_string(&response).expect("serialize large frame");

            let dial_token = token.clone();
            let dialer = std::thread::spawn(move || {
                let mut stream = TcpStream::connect(addr).expect("dial listener");
                stream
                    .write_all(format!("{dial_token}\n").as_bytes())
                    .expect("write token");
                stream
                    .write_all(frame_line.as_bytes())
                    .expect("write frame");
                stream.write_all(b"\n").expect("write frame newline");
            });

            let (_write_half, mut read_half) =
                accept_and_handshake(listener, &child, &token, Duration::from_secs(1))
                    .expect("handshake must succeed");

            // recv() BEFORE join(): a >1 MB write may block the dialer until the
            // reader drains the socket, so joining first could deadlock.
            let received = read_half.recv();
            dialer.join().expect("dialer thread panicked");

            match received {
                Ok(resp) => {
                    assert_eq!(resp.id, 7, "id must round-trip on a large frame");
                    match resp.msg {
                        FromEngine::Error { message, .. } => assert_eq!(
                            message.len(),
                            2_000_000,
                            "the full >1MB single-line frame must survive read_line intact \
                             (no truncation / mis-split); got {} bytes",
                            message.len()
                        ),
                        other => panic!("expected FromEngine::Error, got {other:?}"),
                    }
                }
                Err(e) => panic!(
                    "a legitimate >1MB single-line frame must parse to Ok(Response); got {e:?}"
                ),
            }

            if let Some(mut c) = child.lock().unwrap().take() {
                let _ = c.kill();
            }
        });
    }

    // -----------------------------------------------------------------------
    // spawn_into_tcp — H-DRAIN + H-SPAWN(a) + H-ACCEPT try_wait branch
    // (Phase 1a.6 seam #4)
    // -----------------------------------------------------------------------

    /// A child that reads exactly one line from stdin and echoes it to
    /// stderr, then exits WITHOUT ever dialing back. Portable per the
    /// cross-platform rule: unix uses `sh -c 'head -n1 >&2'`, everything
    /// else falls back to a one-line PowerShell script.
    #[cfg(unix)]
    fn echo_stdin_to_stderr_cmd() -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("head -n1 >&2");
        cmd
    }

    #[cfg(not(unix))]
    fn echo_stdin_to_stderr_cmd() -> Command {
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-Command",
            "$l=[Console]::In.ReadLine(); [Console]::Error.WriteLine($l)",
        ]);
        cmd
    }

    // Drives the REAL production gate (`ensure_started_inner`) with an
    // injected `init` that calls the REAL `spawn_into_tcp`. The child echoes
    // the handshake token to stderr and exits without dialing — this fires
    // THREE hunks in one assertion:
    //   - H-SPAWN(a): the token must actually be written to the child's
    //     stdin for it to have anything to echo.
    //   - H-DRAIN: the drain threads must be running *during* the handshake
    //     so the echoed token reaches `recent_stderr` before the accept
    //     fails — not started only on the (never-taken) success path.
    //   - H-ACCEPT try_wait branch: `accept_and_handshake` must notice the
    //     dead child on `WouldBlock` and fail fast, rather than waiting out
    //     the (long) deadline.
    // A bare "stderr is non-empty" check would be vacuous against H-SPAWN(a)
    // and H-DRAIN; asserting the *exact generated token* appears in the
    // error message binds all three.
    #[test]
    fn test_spawn_into_tcp_child_dies_after_echoing_token() {
        use std::sync::atomic::AtomicUsize;

        watchdog(Duration::from_secs(2), || {
            let host = TsEngineHost::new(make_host_global_config());
            let token = uuid::Uuid::new_v4().to_string();
            let child_slot = Arc::clone(&host.child);
            let recent_stderr = Arc::clone(&host.recent_stderr);
            let call_count = Arc::new(AtomicUsize::new(0));

            let start = Instant::now();
            let result = {
                let call_count = Arc::clone(&call_count);
                let token = token.clone();
                host.ensure_started_inner(move || {
                    call_count.fetch_add(1, Ordering::Relaxed);
                    let listener =
                        TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
                    let cmd = echo_stdin_to_stderr_cmd();
                    let (transport, read_half, stderr, stdout) = spawn_into_tcp(
                        cmd,
                        child_slot,
                        listener,
                        &token,
                        Duration::from_secs(10),
                        recent_stderr,
                    )?;
                    Ok((
                        transport as Arc<dyn EngineTransport>,
                        Box::new(read_half) as Box<dyn EngineReadHalf>,
                        StartedDrains::Tcp { stderr, stdout },
                    ))
                })
            };
            let elapsed = start.elapsed();

            let err = result.expect_err("a child dying before dialing back must return Err");
            let message = err.to_string();
            assert!(
                message.contains(&token),
                "error must carry the echoed token via the drained stderr ring: {message:?}"
            );
            assert!(
                elapsed < Duration::from_secs(2),
                "the try_wait fast-path must fire well before the 10s deadline, took {elapsed:?}"
            );
            assert_eq!(
                host.spawn_count(),
                0,
                "a failed init must not advance spawn_count"
            );

            // Second call, trivially-failing init: no fast-path short-circuit
            // survives a failed spawn — init must re-enter.
            let call_count2 = Arc::clone(&call_count);
            let _ = host.ensure_started_inner(move || {
                call_count2.fetch_add(1, Ordering::Relaxed);
                Err(ExecutionError::other("trivially-failing init"))
            });

            assert_eq!(
                call_count.load(Ordering::Relaxed),
                2,
                "both ensure_started_inner calls must re-enter init (no post-failure short-circuit)"
            );
        });
    }

    // -----------------------------------------------------------------------
    // TcpTransport::shutdown — H-SHUTDOWN (Phase 1a.6 seam #5)
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: the `guard.shutdown(Shutdown::Write)` half-close in
    // `TcpTransport::shutdown`. Reverting it (keep only the best-effort
    // Shutdown-frame send) means our write side never closes, so the peer's
    // drain loop below never observes EOF, never returns, never drops its
    // stream — so OUR OWN reader thread never observes EOF either. Both
    // sides then block on `read_line` forever: `host.shutdown()`'s
    // `reader.join()` hangs → watchdog RED (not a normal assertion failure).
    #[test]
    fn test_tcp_shutdown_graceful() {
        watchdog(Duration::from_secs(10), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let addr = listener.local_addr().expect("local_addr");

            // Peer: stand-in for the engine-host child. Accepts, echoes
            // exactly one response, then keeps its read side open — the
            // ONLY thing that unblocks its next read is OUR half-close
            // (H-SHUTDOWN). Once it observes clean EOF, it returns, which
            // drops its own stream handles — closing the connection fully,
            // so OUR reader (below) then observes EOF too.
            let peer = std::thread::spawn(move || {
                let (stream, _) = listener.accept().expect("peer accept");
                let mut reader = BufReader::new(stream.try_clone().expect("peer try_clone"));
                let mut writer = stream;

                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .expect("peer read_line (request)");
                let request: Request =
                    serde_json::from_str(line.trim_end_matches('\n').trim_end_matches('\r'))
                        .expect("peer parse request");
                let response = Response {
                    id: request.id,
                    msg: make_loaded_response("engine-shutdown"),
                };
                let out = serde_json::to_string(&response).expect("peer serialize response");
                writer
                    .write_all(out.as_bytes())
                    .expect("peer write response");
                writer.write_all(b"\n").expect("peer write newline");
                writer.flush().expect("peer flush");

                // Drain everything else (including the Shutdown frame) until
                // clean EOF — which can ONLY come from our half-close.
                loop {
                    let mut drain = String::new();
                    match reader.read_line(&mut drain) {
                        Ok(0) => break,
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                // Dropping `reader`/`writer` here closes the peer's last
                // handles to the socket, so our reader (below) sees EOF too.
            });

            let stream = TcpStream::connect(addr).expect("connect to peer");
            let write: Arc<dyn EngineTransport> = Arc::new(TcpTransport {
                stream: Mutex::new(stream.try_clone().expect("try_clone write half")),
            });
            let read: Box<dyn EngineReadHalf> = Box::new(TcpReadHalf {
                read: BufReader::new(stream.try_clone().expect("try_clone read half")),
            });

            let host = TsEngineHost::with_transport(write, read, make_host_global_config());

            // Real round trip first — proves the transport is genuinely
            // wired end-to-end, not a trivial no-op.
            let cancel = Cancellation::new();
            let result = host.request(
                make_load_engine_msg(),
                Some(Duration::from_secs(5)),
                &cancel,
            );
            assert!(
                matches!(result, Ok(FromEngine::Loaded { .. })),
                "setup round trip must succeed before testing shutdown: {result:?}"
            );

            let start = Instant::now();
            let shutdown_result = host.shutdown();
            let elapsed = start.elapsed();

            assert!(
                shutdown_result.is_ok(),
                "host.shutdown() must return Ok, got: {shutdown_result:?}"
            );
            assert!(
                elapsed < Duration::from_secs(1),
                "graceful TCP shutdown must complete promptly (the half-close must reach \
                 the peer and round-trip back to EOF), took {elapsed:?}"
            );

            peer.join().expect("peer thread panicked");
        });
    }

    // -----------------------------------------------------------------------
    // TCP large-payload deadlock-freedom (Phase 1a.6 seam #10)
    // -----------------------------------------------------------------------
    //
    // Invariant, not a revert hunk: `TcpTransport` (write half) and
    // `TcpReadHalf` (read half) are independent clones of the accepted
    // stream, and `reader_loop` runs on its own thread — so a large
    // outbound `send()` that blocks on a full send buffer must never
    // prevent the reader thread from draining incoming bytes. A future
    // refactor that serialized send/recv onto one shared path (e.g. one
    // mutex guarding both directions) would deadlock the moment a
    // request's wire size exceeds the socket buffers. The watchdog IS the
    // assertion here: there is no revert hunk to point at, because the
    // failure mode is a hang, not a wrong value.
    //
    // Anti-vacuity guard #1: Linux loopback autotunes send/recv buffers
    // into the megabytes, so a naively fixed payload would sail through
    // even a broken implementation. We shrink both sides via `socket2`
    // and read the EFFECTIVE size back (the kernel doubles/clamps
    // whatever we request), then size the payload comfortably above the
    // observed combined buffer.
    //
    // Anti-vacuity guard #2: the peer withholds reads for ~300ms after
    // accept — long enough for the sender's `write_all` to fill the
    // shrunk send buffer and block — before it starts draining. A peer
    // that reads immediately would let even a buggy whole-message-
    // buffered `send()` complete before backpressure ever mattered.
    #[test]
    fn test_tcp_large_payload_no_deadlock() {
        use socket2::{Domain, SockRef, Socket, Type};

        const SHRUNK_BUF: usize = 4096;

        watchdog(Duration::from_secs(15), || {
            // The recv buffer MUST be shrunk BEFORE the connection is
            // established: SO_RCVBUF governs the TCP receive window advertised
            // during the handshake, so setting it post-accept (as an earlier
            // draft did) reports a small value back but does NOT shrink the
            // effective window — on macOS a 256 KB write then completes without
            // ever blocking, making the whole back-pressure test vacuous.
            // Build the listener via socket2 with SO_RCVBUF set before
            // bind+listen; the accepted socket inherits it.
            let listener: TcpListener = {
                let sock =
                    Socket::new(Domain::IPV4, Type::STREAM, None).expect("socket2 listener socket");
                sock.set_recv_buffer_size(SHRUNK_BUF)
                    .expect("listener set_recv_buffer_size (pre-bind)");
                let bind_addr: std::net::SocketAddr =
                    "127.0.0.1:0".parse().expect("parse bind addr");
                sock.bind(&bind_addr.into()).expect("socket2 bind");
                sock.listen(16).expect("socket2 listen");
                sock.into()
            };
            let addr = listener.local_addr().expect("local_addr");

            // Peer reports its effective (inherited) recv-buffer size back to
            // the main thread before withholding reads.
            let (recv_size_tx, recv_size_rx) = mpsc::sync_channel::<usize>(1);

            let peer = std::thread::spawn(move || {
                let (stream, _) = listener.accept().expect("peer accept");
                let effective_recv = SockRef::from(&stream)
                    .recv_buffer_size()
                    .expect("peer recv_buffer_size");
                recv_size_tx
                    .send(effective_recv)
                    .expect("report effective recv buffer size");

                // Guard #2: withhold reads long enough for the sender's
                // write_all to fill the shrunk send buffer and block.
                std::thread::sleep(Duration::from_millis(300));

                let mut reader = BufReader::new(stream.try_clone().expect("peer try_clone"));
                let mut writer = stream;

                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .expect("peer read_line (large request)");
                let request: Request =
                    serde_json::from_str(line.trim_end_matches('\n').trim_end_matches('\r'))
                        .expect("peer parse large request");

                let response = Response {
                    id: request.id,
                    msg: make_loaded_response("engine-large-payload"),
                };
                let out = serde_json::to_string(&response).expect("peer serialize response");
                writer
                    .write_all(out.as_bytes())
                    .expect("peer write response");
                writer.write_all(b"\n").expect("peer write newline");
                writer.flush().expect("peer flush");
            });

            // Likewise set SO_SNDBUF before connect so the sender's local
            // buffering is small too.
            let stream: TcpStream = {
                let sock =
                    Socket::new(Domain::IPV4, Type::STREAM, None).expect("socket2 client socket");
                sock.set_send_buffer_size(SHRUNK_BUF)
                    .expect("client set_send_buffer_size (pre-connect)");
                sock.connect(&addr.into()).expect("socket2 connect");
                sock.into()
            };
            let effective_send = SockRef::from(&stream)
                .send_buffer_size()
                .expect("sender send_buffer_size");

            let effective_recv = recv_size_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("peer never reported its effective recv buffer size");

            let effective_combined = effective_send + effective_recv;
            let payload_size = (256 * 1024).max(8 * effective_combined);

            eprintln!(
                "test_tcp_large_payload_no_deadlock: effective_send={effective_send} \
                 effective_recv={effective_recv} effective_combined={effective_combined} \
                 payload_size={payload_size}"
            );
            // Non-vacuity guard #1: the payload must comfortably exceed the
            // *measured* effective combined buffer so write_all provably blocks
            // mid-write (the peer withholds reads, so the window cannot grow).
            // Setting SO_*BUF before connect/listen is what makes this read-back
            // truthful: an earlier draft set them post-connect, read back a
            // misleading 4096, and sized the payload too small to exceed the
            // real (unshrunk) window — so the write completed and the test was
            // vacuous. macOS clamps the buffers up (won't honor a few KB), but
            // the 8x-over-measured payload still forces a block. The margin is
            // generous (8x) because with the peer not draining the window
            // cannot autotune upward.
            assert!(
                payload_size >= 8 * effective_combined,
                "payload ({payload_size} bytes) must be >= 8x the effective combined \
                 socket buffer ({effective_combined} bytes) to force a blocking write, \
                 or this test is vacuous"
            );

            let write: Arc<dyn EngineTransport> = Arc::new(TcpTransport {
                stream: Mutex::new(stream.try_clone().expect("try_clone write half")),
            });
            let read: Box<dyn EngineReadHalf> = Box::new(TcpReadHalf {
                read: BufReader::new(stream.try_clone().expect("try_clone read half")),
            });

            let host = TsEngineHost::with_transport(write, read, make_host_global_config());

            // A large `engine_path` string is enough to blow the wire size
            // past the shrunk buffers — `LoadEngine`'s single `String`
            // field is the simplest carrier available for this purpose.
            let large_payload = "A".repeat(payload_size);
            let cancel = Cancellation::new();
            let result = host.request(
                ToEngine::LoadEngine {
                    engine_path: large_payload,
                },
                Some(Duration::from_secs(10)),
                &cancel,
            );

            assert!(
                matches!(result, Ok(FromEngine::Loaded { .. })),
                "large-payload round trip must complete despite the send blocking \
                 mid-write — a refactor that serialized send/recv onto one path would \
                 deadlock here instead: {result:?}"
            );

            peer.join().expect("peer thread panicked");
        });
    }

    // -----------------------------------------------------------------------
    // handle_crash — H-CRASH-REAP (Phase 1a.6 seam #5r, GLOBAL fix)
    // -----------------------------------------------------------------------
    //
    // Named revert hunk: the `c.kill()` call added ahead of `c.wait()` in
    // `handle_crash`. Reverting to a bare `wait()` (no prior `kill()`) means
    // `handle_crash` blocks on the LIVE child until it exits on its own — 30
    // real seconds for `spawn_long_lived_child(30)` — well past this test's
    // watchdog bound → watchdog RED.
    //
    // Vacuity guard: the child MUST stay alive while only the socket closes
    // (a `sleep 30`/`Start-Sleep`, never touched by the socket close below)
    // — otherwise a no-kill `wait()` would return anyway (child already
    // exited) and this test would pass regardless of the fix.
    #[test]
    fn test_tcp_crash_reap_kills_live_child() {
        watchdog(Duration::from_secs(2), || {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
            let addr = listener.local_addr().expect("local_addr");

            // Peer: connects, then closes ONLY the socket — no child process
            // involvement on the peer side at all. This is what drives our
            // reader to observe EOF (a "crash", since `shutting_down` stays
            // false below).
            let peer = std::thread::spawn(move || {
                let (stream, _) = listener.accept().expect("peer accept");
                let _ = stream.shutdown(Shutdown::Both);
            });

            let stream = TcpStream::connect(addr).expect("connect to peer");
            let read_half = TcpReadHalf {
                read: BufReader::new(stream.try_clone().expect("try_clone read half")),
            };
            peer.join().expect("peer thread panicked");

            // The discriminator: a REAL, still-alive child in the shared
            // slot — NOT one that has already exited. It never touches the
            // socket above, so it stays alive independent of the peer's
            // close.
            let child_slot: Arc<Mutex<Option<Child>>> =
                Arc::new(Mutex::new(Some(spawn_long_lived_child(30))));
            let pending: Arc<Mutex<HashMap<u64, PendingSlot>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let recent_stderr: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
            let shutting_down = Arc::new(AtomicBool::new(false));

            let start = Instant::now();
            reader_loop(
                Box::new(read_half),
                pending,
                Arc::clone(&child_slot),
                recent_stderr,
                shutting_down,
            );
            let elapsed = start.elapsed();

            assert!(
                elapsed < Duration::from_secs(2),
                "handle_crash must kill the live child before waiting on it, took {elapsed:?}"
            );
            assert!(
                child_slot.lock().unwrap().is_none(),
                "handle_crash must reap (take) the child"
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
    // Child: a deno dial-back child that hangs forever after completing the
    // handshake (never replies), so it only dies when `Drop` kills it.
    // Deno-gated (the dial-back is a deno child); CI always has deno.
    // Assert: (a) drop returned within bound and (b) PID is gone (kill -0 fails).
    #[test]
    fn test_drop_reaps_no_hang() {
        if !is_available() {
            return;
        }
        watchdog(Duration::from_secs(30), || {
            // Hang forever via an idle-timer loop — NOT `new Promise(() => {})`,
            // which deno's deadlock detector aborts with "Top-level await promise
            // never resolved" (no pending op keeps the loop alive), making the
            // child exit and get crash-reaped before we capture its PID. A pending
            // timer is a real op, so the child stays alive until Drop kills it.
            let (cmd, _script) = deno_dialback_child(
                "while (true) { await new Promise((r) => setTimeout(r, 3600000)); }",
            );
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
    // in `shutdown`.  Revert → threads not joined / socket not closed → hang.
    //
    // Child: a deno dial-back child that discards frames and exits 0 when the
    // socket read side closes (shutdown's half-close). Deno-gated.
    #[test]
    fn test_graceful_shutdown_joins() {
        if !is_available() {
            return;
        }
        watchdog(Duration::from_secs(30), || {
            let (cmd, _script) = deno_dialback_child(DIALBACK_READ_UNTIL_EOF);
            let host = TsEngineHost::start_with_command(cmd, make_host_global_config())
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
    // Child: same deno dial-back read-until-EOF child. Deno-gated.
    #[test]
    fn test_double_teardown_idempotent() {
        if !is_available() {
            return;
        }
        watchdog(Duration::from_secs(30), || {
            let (cmd, _script) = deno_dialback_child(DIALBACK_READ_UNTIL_EOF);
            let host = TsEngineHost::start_with_command(cmd, make_host_global_config())
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
    // Named revert hunk: newline-delimiter framing in TcpTransport/TcpReadHalf.
    // Revert framing → N distinct ids fail to round-trip → assertion fails.
    //
    // Child: a deno dial-back echo child that reads Request frames off the
    // control socket and replies Loaded{name: enginePath} per LoadEngine.
    // SKIPPED in CI only if deno is absent (CI always installs it).
    #[test]
    fn test_real_multiplex_smoke_deno() {
        // Skip if deno is not on PATH.
        if !is_available() {
            return;
        }

        watchdog(Duration::from_secs(30), || {
            // Echo body: reads Request lines off the control socket (`conn`),
            // echoes Loaded{name: enginePath} back for each LoadEngine, exits on
            // Shutdown. Uses the `conn`/`dec`/`enc` bindings the preamble leaves
            // in scope.
            let body = r#"
const buf = new Uint8Array(1024 * 64);
let pending = "";
while (true) {
  const n = await conn.read(buf);
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
        msg: { type: "loaded", discovery: { name: req.msg.enginePath, validExtensions: [] } }
      }) + "\n";
      await conn.write(enc.encode(resp));
    } else if (req.msg && req.msg.type === "shutdown") {
      Deno.exit(0);
    }
  }
}
Deno.exit(0);
"#;
            let (cmd, _script) = deno_dialback_child(body);

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
    // Child: a deno dial-back child that writes a stderr line, sleeps 0.3s,
    // then SIGKILLs itself (so ExitStatus::code() == None). It dials back but
    // never replies on the socket, so any in-flight request blocks until the
    // crash. Deno-gated.
    #[test]
    fn test_real_crash_reaps_and_reports() {
        if !is_available() {
            return;
        }
        watchdog(Duration::from_secs(30), || {
            let (cmd, _script) = deno_dialback_child(
                r#"
console.error("fatal: boom");
await new Promise((r) => setTimeout(r, 300));
Deno.kill(Deno.pid, "SIGKILL");
// Idle-timer loop (a real pending op), NOT `new Promise(() => {})`: if SIGKILL
// is momentarily delayed, deno's deadlock detector would otherwise abort with
// "Top-level await promise never resolved" and exit code 1, breaking the
// code==None (killed-by-signal) assertion. Wait on a timer until the signal lands.
while (true) { await new Promise((r) => setTimeout(r, 3600000)); }
"#,
            );
            let host = Arc::new(
                TsEngineHost::start_with_command(cmd, make_host_global_config())
                    .expect("start_with_command failed"),
            );

            // Spawn a worker that issues a request — it will never get a reply
            // because the child never writes back on the control socket.
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
    // Child: a deno dial-back read-until-EOF child — stays alive until the
    // socket read side closes (i.e. until shutdown). Deno-gated.
    #[test]
    fn test_is_alive_lifecycle_real_spawn() {
        if !is_available() {
            return;
        }
        watchdog(Duration::from_secs(30), || {
            let (cmd, _script) = deno_dialback_child(DIALBACK_READ_UNTIL_EOF);
            let host = TsEngineHost::start_with_command(cmd, make_host_global_config())
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
    // Gate: skip if Deno is absent.  Uses a trivial dial-back read-until-EOF
    // child so we exercise the real spawn path via `start_with_command`.
    #[test]
    fn test_spawn_count_increments_once() {
        if !is_available() {
            return;
        }
        watchdog(Duration::from_secs(30), || {
            let (cmd, _script) = deno_dialback_child(DIALBACK_READ_UNTIL_EOF);
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
