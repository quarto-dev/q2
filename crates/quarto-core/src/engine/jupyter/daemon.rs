/*
 * engine/jupyter/daemon.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * In-process Jupyter kernel daemon.
 */

//! In-process daemon for managing Jupyter kernel sessions.
//!
//! The `JupyterDaemon` manages kernel lifecycle:
//! - Starting kernels on demand, one per document (bd-i7yz08uh)
//! - Giving a document exclusive use of its kernel for its whole run
//! - Shutting a document's kernel down when the document finishes, or
//!   keeping it warm for the next render of that document
//! - Cleaning up on shutdown
//!
//! # One kernel per document (bd-i7yz08uh)
//!
//! Sessions are keyed by (kernel name, working directory, document). Keyed
//! without the document, every document of a project shared one kernel (the
//! working directory is the project directory): a parallel render interleaved
//! two documents' cells on it, and even a serial render let one document read
//! another's globals. Quarto 1 never shares a kernel between documents.
//!
//! A document takes a [`SessionLease`] on its session with
//! [`JupyterDaemon::acquire`] and holds it until it has run every cell, so
//! another render of the same document (a preview re-render) waits for it. Each
//! session has its own lock: documents on different kernels run in parallel.
//! When the document is done, [`JupyterDaemon::finish`] applies the
//! [`Retention`] its caller chose: `q2 render` shuts the kernel down at once
//! (at most `QUARTO_JOBS` kernels are alive), `q2 preview` keeps it warm for
//! the next render of that document, with its state, as Quarto 1's keepalive
//! daemon does.
//!
//! # Kernel scopes (bd-hxhnnlzs)
//!
//! The daemon is a process-global static, and Rust never drops statics
//! — so nothing implicit ever shuts these kernels down. Left alone,
//! every spawned kernel outlives the process, reparents to PID 1, and
//! idles forever (2338 of them accumulated on one dev machine).
//!
//! Lifetime is therefore managed explicitly with refcounted **kernel
//! scopes**: every execution path that can spawn a kernel holds a
//! [`KernelScope`] (the jupyter engine's `execute_qmd` acquires one
//! around each engine run, so no caller can leak by accident), and
//! long-lived callers hold an outer scope for their own lifetime: a
//! `q2 render` invocation a releasing one ([`kernel_scope`]), the
//! `q2 preview` server a retaining one ([`kernel_scope_retaining`]), which
//! is what keeps documents' kernels warm across renders. When the last
//! scope drops, all sessions are shut down: `shutdown_request`, kill
//! backstop, connection-file removal.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use jupyter_protocol::ConnectionInfo;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

use super::error::{JupyterError, Result};
use super::kernelspec;
use super::session::{KernelSession, SessionKey};

/// One session's slot: `None` until its kernel has started, and again once a
/// releasing [`JupyterDaemon::finish`] has shut it down.
type Slot = AsyncMutex<Option<KernelSession>>;

/// What happens to a document's kernel when the document finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// Shut the kernel down (`q2 render`).
    Release,
    /// Keep the kernel, and its state, for the next render of the same
    /// document (`q2 preview`).
    Retain,
}

/// In-process daemon managing Jupyter kernel sessions.
///
/// The daemon maintains a pool of running kernels, one per
/// (kernel, working directory, document); see the module docs.
pub struct JupyterDaemon {
    /// Every known session. Only held for a lookup or an insert, never
    /// across an await: a document holds its own session's lock instead.
    sessions: std::sync::Mutex<HashMap<SessionKey, Arc<Slot>>>,
    /// Ports handed to kernels that are still starting. Kernels start
    /// concurrently, and a port `peek_ports` found free is only bound once
    /// the kernel is up, so without this two starting kernels could be
    /// handed the same port. (Before bd-i7yz08uh one lock serialized every
    /// startup, which with a kernel per document would serialize a project
    /// render's startups.)
    reserved_ports: std::sync::Mutex<HashSet<u16>>,
}

/// A document's exclusive use of its kernel session, from
/// [`JupyterDaemon::acquire`] until [`JupyterDaemon::finish`] (or drop).
///
/// Dereferences to the [`KernelSession`].
pub struct SessionLease {
    key: SessionKey,
    slot: Arc<Slot>,
    guard: OwnedMutexGuard<Option<KernelSession>>,
}

impl SessionLease {
    /// The key of the leased session.
    pub fn key(&self) -> &SessionKey {
        &self.key
    }
}

impl std::ops::Deref for SessionLease {
    type Target = KernelSession;

    fn deref(&self) -> &KernelSession {
        self.guard
            .as_ref()
            .expect("a lease always holds a started session")
    }
}

impl std::ops::DerefMut for SessionLease {
    fn deref_mut(&mut self) -> &mut KernelSession {
        self.guard
            .as_mut()
            .expect("a lease always holds a started session")
    }
}

/// Ports reserved for one starting kernel; returned to the pool on drop.
struct PortReservation<'a> {
    daemon: &'a JupyterDaemon,
    ports: Vec<u16>,
}

impl Drop for PortReservation<'_> {
    fn drop(&mut self) {
        let mut reserved = self.daemon.lock_reserved_ports();
        for port in &self.ports {
            reserved.remove(port);
        }
    }
}

impl JupyterDaemon {
    /// Create a new daemon with default settings.
    pub fn new() -> Self {
        Self {
            sessions: std::sync::Mutex::new(HashMap::new()),
            reserved_ports: std::sync::Mutex::new(HashSet::new()),
        }
    }

    fn lock_sessions(&self) -> std::sync::MutexGuard<'_, HashMap<SessionKey, Arc<Slot>>> {
        // A panic while the map lock is held cannot leave the map half
        // updated (every critical section is a single insert/remove/read).
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_reserved_ports(&self) -> std::sync::MutexGuard<'_, HashSet<u16>> {
        self.reserved_ports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Whether `slot` is still the map's slot for `key` (a releasing
    /// `finish` removes it while a waiter may still hold a clone).
    fn is_current(&self, key: &SessionKey, slot: &Arc<Slot>) -> bool {
        self.lock_sessions()
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, slot))
    }

    fn remove_if_current(&self, key: &SessionKey, slot: &Arc<Slot>) {
        let mut sessions = self.lock_sessions();
        if sessions
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, slot))
        {
            sessions.remove(key);
        }
    }

    /// Lease the kernel session of `document`, starting its kernel if it has
    /// none.
    ///
    /// The lease is exclusive: another `acquire` for the same document waits
    /// until this one is finished or dropped. Leases of different documents
    /// never wait on each other.
    ///
    /// A new kernel is started in `working_dir` with `extra_env` applied on
    /// top of the inherited environment — project `_environment` pairs
    /// pre-filtered so the real environment wins. `extra_env` is a
    /// **spawn-time** input only: the key excludes it, so a retained session
    /// keeps the env it was started with (a preview serves one project, so
    /// the env is stable for a key's lifetime).
    pub async fn acquire(
        &self,
        kernel_name: &str,
        working_dir: &Path,
        document: &Path,
        extra_env: &[(String, String)],
    ) -> Result<SessionLease> {
        let key = SessionKey::new(kernel_name, working_dir, document);
        loop {
            let slot = self
                .lock_sessions()
                .entry(key.clone())
                .or_insert_with(|| Arc::new(AsyncMutex::new(None)))
                .clone();
            let mut guard = slot.clone().lock_owned().await;
            // While we waited, the previous holder may have finished with
            // `Release`, which shuts the kernel down and retires this slot.
            // Start over on the map's current slot.
            if !self.is_current(&key, &slot) {
                continue;
            }
            if guard.is_none() {
                if active_scopes() == 0 {
                    // Not fatal — but this kernel will live until the process
                    // exits (or until some other scope closes), which is
                    // exactly the leak bd-hxhnnlzs is about. Callers should
                    // hold a `kernel_scope()`.
                    tracing::warn!(
                        kernel = %key.kernel_name,
                        "starting a Jupyter kernel outside any kernel scope; \
                         nothing will shut it down automatically"
                    );
                }
                match self.start_kernel(&key, extra_env).await {
                    Ok(session) => *guard = Some(session),
                    Err(e) => {
                        self.remove_if_current(&key, &slot);
                        return Err(e);
                    }
                }
            }
            return Ok(SessionLease { key, slot, guard });
        }
    }

    /// End a document's lease: shut its kernel down (`Release`) or keep it
    /// warm for the document's next render (`Retain`).
    pub async fn finish(&self, lease: SessionLease, retention: Retention) -> Result<()> {
        match retention {
            Retention::Retain => Ok(()),
            Retention::Release => self.shutdown_lease(lease).await,
        }
    }

    /// Shut the leased kernel down and forget its session.
    pub async fn shutdown_lease(&self, mut lease: SessionLease) -> Result<()> {
        // Retire the slot first, while still holding it: a waiter for the
        // same document then starts a fresh kernel on a new slot.
        self.remove_if_current(&lease.key, &lease.slot);
        match lease.guard.take() {
            Some(mut session) => {
                tracing::info!(kernel = %lease.key.kernel_name,
                    document = %lease.key.document.display(), "Shutting down kernel");
                session.shutdown().await
            }
            None => Ok(()),
        }
    }

    /// Reserve five ports no other starting kernel was handed.
    async fn reserve_ports(&self, ip: IpAddr) -> Result<PortReservation<'_>> {
        const ATTEMPTS: usize = 8;
        for _ in 0..ATTEMPTS {
            let ports = runtimelib::peek_ports(ip, 5)
                .await
                .map_err(|e| JupyterError::PortAllocationFailed(e.to_string()))?;
            let mut reserved = self.lock_reserved_ports();
            if ports.iter().all(|p| !reserved.contains(p)) {
                reserved.extend(ports.iter().copied());
                return Ok(PortReservation {
                    daemon: self,
                    ports,
                });
            }
        }
        Err(JupyterError::PortAllocationFailed(format!(
            "no five free ports outside those of kernels still starting after {ATTEMPTS} attempts"
        )))
    }

    /// Start a new kernel for the given key.
    async fn start_kernel(
        &self,
        key: &SessionKey,
        extra_env: &[(String, String)],
    ) -> Result<KernelSession> {
        tracing::info!(kernel = %key.kernel_name, dir = %key.working_dir.display(),
            document = %key.document.display(), "Starting Jupyter kernel");

        // 1. Find kernelspec
        let kernel = kernelspec::find_kernelspec(&key.kernel_name).await?;

        // 2. Allocate ports for ZeroMQ channels, held until the kernel has
        //    bound them (it is ready).
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let reservation = self.reserve_ports(ip).await?;
        let ports = &reservation.ports;

        // 3. Build connection info
        let connection_info = ConnectionInfo {
            transport: jupyter_protocol::connection_info::Transport::TCP,
            ip: ip.to_string(),
            stdin_port: ports[0],
            control_port: ports[1],
            hb_port: ports[2],
            shell_port: ports[3],
            iopub_port: ports[4],
            signature_scheme: "hmac-sha256".to_string(),
            key: Uuid::new_v4().to_string(),
            kernel_name: Some(key.kernel_name.clone()),
        };

        // 4. Write connection file
        let runtime_dir = runtimelib::dirs::runtime_dir();
        tokio::fs::create_dir_all(&runtime_dir).await.map_err(|e| {
            JupyterError::ConnectionFileError {
                path: runtime_dir.clone(),
                message: e.to_string(),
            }
        })?;

        let connection_file = runtime_dir.join(format!("kernel-{}.json", Uuid::new_v4()));
        let connection_json = serde_json::to_string(&connection_info)?;
        tokio::fs::write(&connection_file, &connection_json)
            .await
            .map_err(|e| JupyterError::ConnectionFileError {
                path: connection_file.clone(),
                message: e.to_string(),
            })?;

        // 5. Spawn kernel process
        let mut spec = kernel.spec.clone();
        expand_resource_dir(&mut spec);
        let process = spec
            .command(&connection_file, None, None)
            .map_err(|e| JupyterError::ProcessSpawnError {
                kernel: key.kernel_name.clone(),
                message: e.to_string(),
            })?
            .current_dir(&key.working_dir)
            .envs(extra_env.iter().map(|(k, v)| (k, v)))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| JupyterError::ProcessSpawnError {
                kernel: key.kernel_name.clone(),
                message: e.to_string(),
            })?;

        // 6. Create ZeroMQ socket connections
        let session_id = Uuid::new_v4().to_string();

        let peer_identity = runtimelib::peer_identity_for_session(&session_id).map_err(|e| {
            JupyterError::SocketError {
                socket_type: "shell".to_string(),
                message: e.to_string(),
            }
        })?;
        let shell_socket = runtimelib::create_client_shell_connection_with_identity(
            &connection_info,
            &session_id,
            peer_identity,
        )
        .await
        .map_err(|e| JupyterError::SocketError {
            socket_type: "shell".to_string(),
            message: e.to_string(),
        })?;

        let iopub_socket =
            runtimelib::create_client_iopub_connection(&connection_info, "", &session_id)
                .await
                .map_err(|e| JupyterError::SocketError {
                    socket_type: "iopub".to_string(),
                    message: e.to_string(),
                })?;

        // 7. Create session
        let mut session = KernelSession {
            kernel,
            process,
            connection_info,
            connection_file,
            shell_socket,
            iopub_socket,
            session_id,
            execution_count: 0,
            working_dir: key.working_dir.clone(),
        };

        // 8. Wait for kernel to become ready. Nothing else can reach this
        //    session yet, so no lock is held while waiting.
        match session.wait_for_ready().await {
            Ok(info) => {
                tracing::info!(
                    kernel = %key.kernel_name,
                    language = %info.language,
                    "Kernel is ready"
                );
            }
            Err(e) => {
                tracing::error!(kernel = %key.kernel_name, error = %e, "Kernel failed to start");
                let _ = session.shutdown().await;
                return Err(e);
            }
        }
        drop(reservation);

        tracing::info!(kernel = %key.kernel_name, "Kernel started successfully");

        Ok(session)
    }

    /// Shutdown all kernel sessions.
    ///
    /// Waits for any outstanding lease to end first.
    pub async fn shutdown_all(&self) -> Result<()> {
        let slots: Vec<(SessionKey, Arc<Slot>)> = self.lock_sessions().drain().collect();
        for (key, slot) in slots {
            if let Some(mut session) = slot.lock().await.take() {
                tracing::info!(kernel = %key.kernel_name, "Shutting down kernel");
                let _ = session.shutdown().await;
            }
        }
        Ok(())
    }

    /// Shutdown all kernel sessions without needing an async context.
    ///
    /// This is the teardown path of the last [`KernelScope`] (which
    /// runs in `Drop`, hence sync). A session can only be leased there
    /// if a document is mid-run while its scope closes, which scopes
    /// prevent by outliving the executions they cover; if it ever
    /// happens, warn rather than block or panic — the per-`Child`
    /// `kill_on_drop` remains as the final backstop when the lease drops.
    pub fn shutdown_all_blocking(&self) {
        let slots: Vec<(SessionKey, Arc<Slot>)> = self.lock_sessions().drain().collect();
        for (key, slot) in slots {
            let Ok(mut guard) = slot.try_lock() else {
                tracing::warn!(
                    kernel = %key.kernel_name,
                    "kernel session leased during scope teardown; \
                     skipping shutdown (kill-on-drop remains as backstop)"
                );
                continue;
            };
            if let Some(mut session) = guard.take() {
                tracing::info!(kernel = %key.kernel_name, "Shutting down kernel");
                session.shutdown_blocking();
            }
        }
    }

    /// The number of sessions the daemon knows (started, or starting).
    pub fn session_count(&self) -> usize {
        self.lock_sessions().len()
    }

    /// Check if a session exists for the given key.
    pub fn has_session(&self, key: &SessionKey) -> bool {
        self.lock_sessions().contains_key(key)
    }
}

impl Default for JupyterDaemon {
    fn default() -> Self {
        Self::new()
    }
}

/// Global daemon instance for kernel management.
///
/// Using a lazy static ensures the daemon persists across multiple renders.
static DAEMON: std::sync::OnceLock<Arc<JupyterDaemon>> = std::sync::OnceLock::new();

/// Get the global daemon instance.
pub fn daemon() -> Arc<JupyterDaemon> {
    DAEMON
        .get_or_init(|| Arc::new(JupyterDaemon::new()))
        .clone()
}

/// Long-lived runtime that owns every kernel session's resources.
///
/// Kernel ZeroMQ sockets and the kernel `Child` are tokio resources
/// bound to the runtime they were created on. Sessions outlive a
/// single engine run (that's the point of the daemon), so they must
/// not be created on a per-call runtime: reusing a session whose
/// runtime was dropped fails with "A Tokio 1.x context was found, but
/// it is being shutdown". Before bd-hxhnnlzs this was masked by the
/// startup race — every "reuse" actually re-spawned a fresh kernel and
/// evicted (killed) the old one.
///
/// Engine runs drive their futures with `block_on` from their own
/// (rayon) threads, so documents run concurrently without needing more
/// than one worker here; the worker serves the I/O and timer drivers.
/// Being a static, the runtime is never dropped; kernels are shut down
/// by leases and the [`KernelScope`] machinery, not by runtime teardown.
static ENGINE_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

/// The shared engine runtime. All daemon session operations —
/// spawning, executing, async shutdown — must run on it.
pub(crate) fn engine_runtime() -> &'static tokio::runtime::Runtime {
    ENGINE_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("jupyter-daemon")
            .enable_all()
            .build()
            .expect("building the shared Jupyter engine runtime")
    })
}

/// Number of currently-open [`KernelScope`]s.
static ACTIVE_SCOPES: AtomicUsize = AtomicUsize::new(0);

/// Number of currently-open retaining [`KernelScope`]s.
static RETAINING_SCOPES: AtomicUsize = AtomicUsize::new(0);

fn active_scopes() -> usize {
    ACTIVE_SCOPES.load(Ordering::SeqCst)
}

/// What a document's kernel should do when the document finishes: kept
/// warm while any retaining scope is open (`q2 preview`), shut down
/// otherwise (`q2 render`, tests, any other caller).
pub(crate) fn retention() -> Retention {
    if RETAINING_SCOPES.load(Ordering::SeqCst) > 0 {
        Retention::Retain
    } else {
        Retention::Release
    }
}

/// RAII handle keeping the global kernel pool alive (bd-hxhnnlzs).
///
/// When the last open scope drops, every session in the global daemon
/// is shut down ([`JupyterDaemon::shutdown_all_blocking`]). The
/// jupyter engine acquires one around each engine run, so kernels
/// never outlive the work that spawned them. A scope also states a
/// policy (bd-i7yz08uh): while a *retaining* scope
/// ([`kernel_scope_retaining`]) is open, a document's kernel stays warm
/// for the next render of that document; otherwise it is shut down as
/// soon as the document finishes.
///
/// Scope drops must happen on the normal return path — `Drop` does not
/// run across `std::process::exit`, so hold scopes tightly around the
/// work rather than around exit-calling code.
#[must_use = "the kernel pool shuts down when the last scope drops; \
              bind to a named guard for the intended lifetime"]
pub struct KernelScope {
    retaining: bool,
}

/// Open a [`KernelScope`] that shuts each document's kernel down when
/// the document finishes (`q2 render`).
pub fn kernel_scope() -> KernelScope {
    ACTIVE_SCOPES.fetch_add(1, Ordering::SeqCst);
    KernelScope { retaining: false }
}

/// Open a [`KernelScope`] that keeps each document's kernel warm, with
/// its state, for the next render of that document (`q2 preview`, as
/// Quarto 1's keepalive daemon).
pub fn kernel_scope_retaining() -> KernelScope {
    ACTIVE_SCOPES.fetch_add(1, Ordering::SeqCst);
    RETAINING_SCOPES.fetch_add(1, Ordering::SeqCst);
    KernelScope { retaining: true }
}

impl Drop for KernelScope {
    fn drop(&mut self) {
        if self.retaining {
            RETAINING_SCOPES.fetch_sub(1, Ordering::SeqCst);
        }
        let was = ACTIVE_SCOPES.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(was > 0, "KernelScope refcount underflow");
        // Last scope out shuts down the pool. A concurrent
        // `kernel_scope()` call racing this transition could get its
        // fresh kernel torn down; in practice every spawning path runs
        // under a scope that is still open here, and the cost of the
        // race is a kernel restart, not a leak.
        if was == 1
            && let Some(daemon) = DAEMON.get()
        {
            daemon.shutdown_all_blocking();
        }
    }
}

/// Expand `{resource_dir}` in a kernelspec's `argv`.
///
/// `runtimelib` substitutes only `{connection_file}`, but the Jupyter
/// kernelspec contract also defines `{resource_dir}` (the kernelspec's own
/// directory), e.g. ggsql's `{resource_dir}/ggsql-jupyter`.
fn expand_resource_dir(spec: &mut runtimelib::KernelspecDir) {
    // Absolutize: the kernel is spawned with a different cwd than the one
    // the (possibly relative, via JUPYTER_PATH) kernelspec was found from.
    let dir = std::path::absolute(&spec.path).unwrap_or_else(|_| spec.path.clone());
    let resource_dir = dir.to_string_lossy().into_owned();
    for arg in &mut spec.kernelspec.argv {
        *arg = arg.replace("{resource_dir}", &resource_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_with_argv(argv: &[&str]) -> runtimelib::KernelspecDir {
        runtimelib::KernelspecDir {
            kernel_name: "k".to_string(),
            path: std::path::PathBuf::from("kernels").join("k"),
            kernelspec: serde_json::from_value(serde_json::json!({
                "argv": argv,
                "display_name": "k",
                "language": "k",
            }))
            .unwrap(),
        }
    }

    #[test]
    fn test_expand_resource_dir_substitutes_argv() {
        let mut spec = spec_with_argv(&["{resource_dir}/bin", "-f", "{connection_file}"]);
        let dir = std::path::absolute(&spec.path)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        expand_resource_dir(&mut spec);
        assert_eq!(
            spec.kernelspec.argv,
            vec![
                format!("{dir}/bin"),
                "-f".to_string(),
                "{connection_file}".to_string()
            ]
        );
    }

    #[test]
    fn test_expand_resource_dir_leaves_other_argv_alone() {
        let mut spec = spec_with_argv(&["python3", "-m", "ipykernel_launcher"]);
        expand_resource_dir(&mut spec);
        assert_eq!(
            spec.kernelspec.argv,
            vec!["python3", "-m", "ipykernel_launcher"]
        );
    }

    #[tokio::test]
    async fn test_daemon_initial_state() {
        let daemon = JupyterDaemon::new();
        assert_eq!(daemon.session_count(), 0);
    }

    #[test]
    fn test_kernel_scope_refcount() {
        // nextest runs each test in its own process, so the global
        // counter starts at 0 and nothing else touches it.
        assert_eq!(active_scopes(), 0);
        let outer = kernel_scope();
        {
            let _inner = kernel_scope();
            assert_eq!(active_scopes(), 2);
        }
        // Inner drop must not tear down while an outer scope is open.
        assert_eq!(active_scopes(), 1);
        drop(outer);
        assert_eq!(active_scopes(), 0);
    }
}
