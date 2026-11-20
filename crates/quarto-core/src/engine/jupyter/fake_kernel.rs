/*
 * engine/jupyter/fake_kernel.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * A kernel-free stand-in for ipykernel, for tests of the Jupyter unit loop.
 */

//! A fake Jupyter kernel, so the unit loop can be tested without `ipykernel`.
//!
//! **Design (EXR-3 Phase 1 spike, `runtimelib` 2.0 read 2026-10-31).** `runtimelib`
//! 2.0 ships the server half of the protocol behind its `test-kernel` feature:
//! `TestKernel::start_from_file` binds the five channels named in a connection file,
//! answers `kernel_info_request`, heartbeat and `execute_request`, and for an
//! execute request sends `status busy`, `execute_input`, the outputs and an
//! `execute_reply`. For code it has a canned response for it sends exactly those
//! outputs (an empty list is a silent execution: the `None` and statement cases);
//! any other code is echoed as stdout. It is a library, not a process.
//!
//! The session under test starts a kernel by *spawning the kernelspec's argv* with a
//! connection file it wrote itself, so the fake needs a process. It is the test
//! executable itself: the kernelspec's argv re-runs this lib's test binary on the one
//! test [`tests::fake_kernel_process_entry`] (libtest treats the trailing connection
//! file path as one more name filter, which `--exact` makes harmless), and that test
//! turns into a kernel when [`SCRIPT_ENV`] is set. In a normal run the variable is
//! unset and the entry test passes at once. No binary target is added to the crate.
//!
//! The canned responses are scripted per test as JSON in [`SCRIPT_ENV`], passed to the
//! child through `JupyterDaemon::acquire`'s `extra_env`. A kernelspec directory is
//! written under a temp dir that `JUPYTER_PATH` points at, so
//! `find_kernelspec` discovers it through the real code path. Setting the variable is
//! process-global; nextest runs each test in its own process, which is what makes
//! that safe (the same precedent as `extension/mod.rs`).
//!
//! **JOL-5 additions.** (1) The kernel's name and language come from its kernelspec,
//! and each kernelspec carries its own script in the environment its session is
//! started with, so one test executable backs several kernelspecs that answer the
//! same code differently ([`answering_as`] builds a script that names the kernel
//! that ran the code, which is how the end-to-end tests observe kernel selection).
//! (2) [`FakeKernelHome::jupyter_path`] names the directory `JUPYTER_PATH` holds.
//! The `write <file>` request (a cross-kernel file handoff) is *not* built: only a
//! reopened JOL-3 (several kernels per document) needs it. The availability probe's
//! stub `jupyter` is not built either: the end-to-end tests call `execute_qmd`
//! directly, which bypasses the probe. EXR-5 reuses all of this.

use std::collections::BTreeMap;
use std::path::Path;

use jupyter_protocol::{
    DisplayData, ErrorOutput, ExecuteResult, ExecutionCount, JupyterMessageContent, Media,
    StreamContent,
};
use runtimelib::{CannedResponse, TestKernel, TestKernelConfig};
use serde::{Deserialize, Serialize};

/// Environment variable that turns the test executable into a kernel; its value is a
/// JSON-encoded [`Script`].
pub(crate) const SCRIPT_ENV: &str = "QUARTO_FAKE_KERNEL_SCRIPT";

/// Like [`SCRIPT_ENV`], but its value is a JSON object `{kernel name: Script}`: the
/// kernel reads its own name from the connection file the session wrote
/// (`kernel_name`) and follows that script. Lets one environment serve several
/// kernelspecs that answer the same code differently (JOL-5).
pub(crate) const SCRIPTS_ENV: &str = "QUARTO_FAKE_KERNEL_SCRIPTS";

/// When set, the kernel process writes the variable's value to stderr and exits
/// with code 3 instead of serving, like a kernel that cannot bind its ports.
pub(crate) const EXIT_ENV: &str = "QUARTO_FAKE_KERNEL_EXIT";

/// When set to `<n>:<path>`, each kernel process appends a line to the file at
/// `<path>` (before any [`EXIT_ENV`] exit, so the file counts every spawn) and,
/// while the file then holds at most `n` lines, exits with code 1 and a
/// port-conflict message instead of serving: the first `n` kernel starts lose
/// a port, later ones serve (bd-e87d5tmn).
pub(crate) const FAIL_FIRST_ENV: &str = "QUARTO_FAKE_KERNEL_FAIL_FIRST";

/// What a [`FAIL_FIRST_ENV`] kernel writes to stderr, as ipykernel does when
/// a port it was handed is taken.
pub(crate) const PORT_CONFLICT_STDERR: &str =
    "zmq.error.ZMQError: Address already in use (addr='tcp://127.0.0.1:38829')";

/// The libtest name of the entry test the kernelspec argv runs.
const ENTRY_TEST: &str = "engine::jupyter::fake_kernel::tests::fake_kernel_process_entry";

/// One output the fake kernel sends for a piece of code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum FakeOutput {
    /// A `stream` message on stdout.
    Stdout(String),
    /// An `execute_result` whose mime bundle is the given `{mime: value}` object.
    Result(serde_json::Value),
    /// A `display_data` with the given mime bundle.
    Display(serde_json::Value),
    /// An `error` message.
    Error { ename: String, evalue: String },
    /// A `stream` message on stdout carrying the kernel process's id and a newline,
    /// so a test can tell which kernel process ran a piece of code (bd-i7yz08uh). The
    /// id is read in the kernel process, when the script is loaded.
    KernelPid,
}

/// Code (exact match) to the outputs the kernel sends for it. Code not in the script
/// is echoed back as stdout, by `TestKernel`'s default.
pub(crate) type Script = BTreeMap<String, Vec<FakeOutput>>;

/// A script in which each of `codes` prints `name` on stdout, so a test can tell which
/// kernelspec ran a piece of code.
pub(crate) fn answering_as(name: &str, codes: &[&str]) -> Script {
    codes
        .iter()
        .map(|c| (c.to_string(), vec![FakeOutput::Stdout(format!("{name}\n"))]))
        .collect()
}

impl FakeOutput {
    /// `execute_result` with a single `text/plain` value.
    pub(crate) fn plain_result(text: &str) -> Self {
        Self::Result(serde_json::json!({ "text/plain": text }))
    }

    fn into_message(self) -> JupyterMessageContent {
        match self {
            Self::Stdout(text) => StreamContent::stdout(&text).into(),
            Self::KernelPid => StreamContent::stdout(&format!("{}\n", std::process::id())).into(),
            Self::Result(bundle) => ExecuteResult {
                execution_count: ExecutionCount::new(1),
                data: serde_json::from_value::<Media>(bundle).expect("valid mime bundle"),
                metadata: Default::default(),
                transient: None,
            }
            .into(),
            Self::Display(bundle) => DisplayData {
                data: serde_json::from_value::<Media>(bundle).expect("valid mime bundle"),
                metadata: Default::default(),
                transient: None,
            }
            .into(),
            Self::Error { ename, evalue } => ErrorOutput {
                ename,
                evalue,
                traceback: vec![],
            }
            .into(),
        }
    }
}

fn config_from_script(script: Script) -> TestKernelConfig {
    script
        .into_iter()
        .fold(TestKernelConfig::new(), |config, (code, outputs)| {
            config.with_response(
                code,
                CannedResponse {
                    outputs: outputs.into_iter().map(FakeOutput::into_message).collect(),
                },
            )
        })
}

/// A temp directory holding fake kernelspecs, found through `JUPYTER_PATH`.
pub(crate) struct FakeKernelHome {
    dir: tempfile::TempDir,
    scripts: std::cell::RefCell<BTreeMap<String, Script>>,
}

/// An installed fake kernel: its kernelspec name and the environment to pass to
/// `JupyterDaemon::acquire` so the spawned process runs the scripted kernel.
pub(crate) struct FakeKernel {
    pub name: String,
    pub env: Vec<(String, String)>,
}

impl FakeKernelHome {
    /// Create the directory and point `JUPYTER_PATH` at it.
    ///
    /// Process-global: only call from a test, and expect nextest's process-per-test
    /// isolation.
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir for fake kernelspecs");
        // SAFETY: tests run one per process under nextest; no other thread reads the
        // environment concurrently at this point in the test.
        unsafe { std::env::set_var("JUPYTER_PATH", dir.path()) };
        Self {
            dir,
            scripts: Default::default(),
        }
    }

    /// Install a kernelspec `name` for `language` whose kernel follows `script`.
    pub(crate) fn install(&self, name: &str, language: &str, script: &Script) -> FakeKernel {
        let spec_dir = self.dir.path().join("kernels").join(name);
        std::fs::create_dir_all(&spec_dir).expect("kernelspec dir");
        let exe = std::env::current_exe().expect("current_exe");
        let spec = serde_json::json!({
            "argv": [exe, "--exact", ENTRY_TEST, "--nocapture", "{connection_file}"],
            "display_name": format!("Fake {name}"),
            "language": language,
        });
        std::fs::write(spec_dir.join("kernel.json"), spec.to_string()).expect("kernel.json");
        self.scripts
            .borrow_mut()
            .insert(name.to_string(), script.clone());
        FakeKernel {
            name: name.to_string(),
            env: vec![(
                SCRIPT_ENV.to_string(),
                serde_json::to_string(script).expect("script json"),
            )],
        }
    }

    /// The environment that makes *every* kernelspec installed so far follow its own
    /// script, whichever of them the session starts.
    pub(crate) fn env_all(&self) -> Vec<(String, String)> {
        vec![(
            SCRIPTS_ENV.to_string(),
            serde_json::to_string(&*self.scripts.borrow()).expect("scripts json"),
        )]
    }

    /// The value `JUPYTER_PATH` was set to: the directory holding `kernels/<name>/`.
    pub(crate) fn jupyter_path(&self) -> &Path {
        self.dir.path()
    }
}

/// Run the fake kernel on the connection file named by the last command-line argument
/// until the process is killed.
fn run_kernel_process(script_json: Option<String>, scripts_json: Option<String>) {
    let connection_file = std::env::args()
        .next_back()
        .expect("connection file argument");
    let script: Script = match (script_json, scripts_json) {
        (Some(json), _) => serde_json::from_str(&json).expect("script json"),
        (None, Some(json)) => {
            let mut all: BTreeMap<String, Script> =
                serde_json::from_str(&json).expect("scripts json");
            let connection: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(&connection_file).expect("connection file"),
            )
            .expect("connection json");
            let name = connection["kernel_name"].as_str().expect("kernel_name");
            all.remove(name).expect("a script for this kernel")
        }
        (None, None) => return,
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("kernel runtime")
        .block_on(async {
            let handle = TestKernel::start_from_file(connection_file, config_from_script(script))
                .await
                .expect("start fake kernel");
            // `TestKernel::run` binds its sockets inside the spawned task, so a
            // port taken before the kernel binds it surfaces here, as an error.
            // Report it and exit non-zero, as ipykernel does: the daemon
            // recognizes a port conflict only by its stderr, and a silent exit 0
            // would skip its retry (bd-0w8rgjdf).
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    eprintln!("fake kernel failed: {e}");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("fake kernel task failed: {e}");
                    std::process::exit(1);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::jupyter::daemon::JupyterDaemon;
    use crate::presenter::output::CellOutput;

    /// Doubles as the kernel's `main`: when [`SCRIPT_ENV`] is set this test never
    /// returns (the kernel runs until killed); otherwise it passes immediately.
    #[test]
    fn fake_kernel_process_entry() {
        // Count the spawn first, so a test can count spawns that then exit.
        let fail_first = std::env::var(FAIL_FIRST_ENV).ok().map(|spec| {
            let (n, path) = spec.split_once(':').expect("<n>:<path>");
            let n: usize = n.parse().expect("failure count");
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("spawn count file");
            writeln!(file, "spawn").expect("count the spawn");
            drop(file);
            let spawns = std::fs::read_to_string(path)
                .expect("spawn count")
                .lines()
                .count();
            spawns <= n
        });
        if let Ok(message) = std::env::var(EXIT_ENV) {
            eprintln!("{message}");
            std::process::exit(3);
        }
        if fail_first == Some(true) {
            eprintln!("{PORT_CONFLICT_STDERR}");
            std::process::exit(1);
        }
        run_kernel_process(
            std::env::var(SCRIPT_ENV).ok(),
            std::env::var(SCRIPTS_ENV).ok(),
        );
    }

    fn echo_script() -> Script {
        Script::from([
            (
                "echo hi".to_string(),
                vec![
                    FakeOutput::Stdout("hi\n".to_string()),
                    FakeOutput::plain_result("'hi'"),
                ],
            ),
            ("1 + 1".to_string(), vec![FakeOutput::plain_result("2")]),
            ("x = 1".to_string(), vec![]),
            (
                "boom".to_string(),
                vec![FakeOutput::Error {
                    ename: "NameError".to_string(),
                    evalue: "name 'boom' is not defined".to_string(),
                }],
            ),
        ])
    }

    #[tokio::test]
    async fn test_fake_kernel_two_names_one_binary() {
        let home = FakeKernelHome::new();
        let a = home.install("fake-a", "alpha", &answering_as("fake-a", &["who"]));
        let b = home.install("fake-b", "beta", &answering_as("fake-b", &["who"]));
        assert!(
            home.jupyter_path()
                .join("kernels/fake-b/kernel.json")
                .is_file()
        );
        let cwd = std::env::current_dir().unwrap();
        let daemon = JupyterDaemon::new();

        let mut said = Vec::new();
        for kernel in [&a, &b] {
            let mut lease = daemon
                .acquire(&kernel.name, &cwd, &cwd.join("doc.qmd"), &home.env_all())
                .await
                .expect("each fake kernelspec starts");
            let r = lease.execute("who").await.unwrap();
            said.push(match r.outputs.as_slice() {
                [CellOutput::Stream { text, .. }] => text.clone(),
                other => panic!("unexpected outputs {other:?}"),
            });
        }
        assert_eq!(said, ["fake-a\n", "fake-b\n"]);
        daemon.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn test_fake_kernel_echo_round_trip() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &echo_script());
        let cwd = std::env::current_dir().unwrap();
        let daemon = JupyterDaemon::new();

        let mut lease = daemon
            .acquire(&kernel.name, &cwd, &cwd.join("doc.qmd"), &kernel.env)
            .await
            .expect("fake kernel starts through the real session path");

        // Scripted: one stream, then the value.
        let r = lease.execute("echo hi").await.unwrap();
        assert!(matches!(
            r.outputs.as_slice(),
            [CellOutput::Stream { name, text }, CellOutput::ExecuteResult { data, .. }]
                if name == "stdout" && text == "hi\n"
                    && data.get("text/plain") == Some(&serde_json::json!("'hi'"))
        ));

        // A silent execution (statement, or a `None` value): no outputs at all.
        let r = lease.execute("x = 1").await.unwrap();
        assert!(r.outputs.is_empty(), "got {:?}", r.outputs);

        // An error message.
        let r = lease.execute("boom").await.unwrap();
        assert!(matches!(
            r.outputs.as_slice(),
            [CellOutput::Error { ename, .. }] if ename == "NameError"
        ));

        // Unscripted code is echoed as stdout (TestKernel's default).
        let r = lease.execute("anything else").await.unwrap();
        assert!(matches!(
            r.outputs.as_slice(),
            [CellOutput::Stream { text, .. }] if text == "anything else"
        ));

        daemon.shutdown_lease(lease).await.unwrap();
    }

    // ── bd-i7yz08uh: leases ─────────────────────────────────────────────

    use super::super::daemon::Retention;
    use std::time::Duration;

    fn pid_script() -> Script {
        Script::from([("pid".to_string(), vec![FakeOutput::KernelPid])])
    }

    async fn pid_of(lease: &mut crate::engine::jupyter::SessionLease) -> String {
        let r = lease.execute("pid").await.unwrap();
        match r.outputs.as_slice() {
            [CellOutput::Stream { text, .. }] => text.clone(),
            other => panic!("unexpected outputs {other:?}"),
        }
    }

    /// A document's lease is exclusive: a second render of the same document
    /// waits until the first one is done, then gets the same (retained) kernel.
    #[tokio::test]
    async fn test_lease_is_exclusive_per_document() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &pid_script());
        let cwd = std::env::current_dir().unwrap();
        let doc = cwd.join("a.qmd");
        let daemon = JupyterDaemon::new();

        let mut first = daemon
            .acquire(&kernel.name, &cwd, &doc, &kernel.env)
            .await
            .unwrap();
        let first_pid = pid_of(&mut first).await;
        let waiting = tokio::time::timeout(
            Duration::from_millis(300),
            daemon.acquire(&kernel.name, &cwd, &doc, &kernel.env),
        )
        .await;
        assert!(
            waiting.is_err(),
            "a second lease on a.qmd's kernel was granted while the first was held"
        );

        daemon.finish(first, Retention::Retain).await.unwrap();
        let mut second = daemon
            .acquire(&kernel.name, &cwd, &doc, &kernel.env)
            .await
            .unwrap();
        assert_eq!(pid_of(&mut second).await, first_pid);
        drop(second);
        daemon.shutdown_all().await.unwrap();
    }

    /// A held lease never blocks another document: a document's whole run
    /// holds its own kernel, not a daemon-wide lock.
    #[tokio::test]
    async fn test_held_lease_does_not_block_another_document() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &pid_script());
        let cwd = std::env::current_dir().unwrap();
        let daemon = JupyterDaemon::new();

        let mut a = daemon
            .acquire(&kernel.name, &cwd, &cwd.join("a.qmd"), &kernel.env)
            .await
            .unwrap();
        let a_pid = pid_of(&mut a).await;
        // `a` stays leased while b.qmd starts its kernel and runs a cell.
        let b_pid = tokio::time::timeout(Duration::from_secs(30), async {
            let mut b = daemon
                .acquire(&kernel.name, &cwd, &cwd.join("b.qmd"), &kernel.env)
                .await
                .unwrap();
            pid_of(&mut b).await
        })
        .await
        .expect("b.qmd ran while a.qmd held its lease");
        assert_ne!(a_pid, b_pid);
        drop(a);
        daemon.shutdown_all().await.unwrap();
    }

    /// `Release` shuts the kernel down and forgets the session; a later lease
    /// on the same document starts a fresh kernel.
    #[tokio::test]
    async fn test_release_shuts_the_kernel_down() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &pid_script());
        let cwd = std::env::current_dir().unwrap();
        let doc = cwd.join("a.qmd");
        let daemon = JupyterDaemon::new();

        let mut lease = daemon
            .acquire(&kernel.name, &cwd, &doc, &kernel.env)
            .await
            .unwrap();
        let first_pid = pid_of(&mut lease).await;
        let key = lease.key().clone();
        daemon.finish(lease, Retention::Release).await.unwrap();
        assert!(!daemon.has_session(&key));
        assert_eq!(daemon.session_count(), 0);

        let mut again = daemon
            .acquire(&kernel.name, &cwd, &doc, &kernel.env)
            .await
            .unwrap();
        assert_ne!(pid_of(&mut again).await, first_pid);
        daemon.shutdown_lease(again).await.unwrap();
    }

    /// A render waiting on a document's lease while the holder releases it
    /// gets a freshly started kernel, tracked by the daemon (not the retired
    /// session).
    #[tokio::test]
    async fn test_waiter_after_release_gets_a_tracked_fresh_kernel() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &pid_script());
        let cwd = std::env::current_dir().unwrap();
        let doc = cwd.join("a.qmd");
        let daemon = JupyterDaemon::new();

        let mut first = daemon
            .acquire(&kernel.name, &cwd, &doc, &kernel.env)
            .await
            .unwrap();
        let first_pid = pid_of(&mut first).await;
        let (second, ()) = tokio::join!(
            daemon.acquire(&kernel.name, &cwd, &doc, &kernel.env),
            async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                daemon.finish(first, Retention::Release).await.unwrap();
            }
        );
        let mut second = second.unwrap();
        assert_ne!(pid_of(&mut second).await, first_pid);
        assert!(daemon.has_session(second.key()));
        daemon.shutdown_lease(second).await.unwrap();
        assert_eq!(daemon.session_count(), 0);
    }

    /// A kernel that exits while starting fails the start at once with its
    /// exit code and stderr, not after the shell connect's 30s timeout.
    #[tokio::test]
    async fn test_kernel_exiting_at_startup_reports_its_stderr() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &pid_script());
        let mut env = kernel.env.clone();
        env.push((
            EXIT_ENV.to_string(),
            "ModuleNotFoundError: No module named 'ipykernel_launcher'".to_string(),
        ));
        let cwd = std::env::current_dir().unwrap();
        let daemon = JupyterDaemon::new();

        let started = std::time::Instant::now();
        let err = match daemon
            .acquire(&kernel.name, &cwd, &cwd.join("a.qmd"), &env)
            .await
        {
            Ok(_) => panic!("a kernel that exits cannot be leased"),
            Err(e) => e,
        };
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "took {:?}",
            started.elapsed()
        );
        match &err {
            crate::engine::jupyter::error::JupyterError::ExitedDuringStartup {
                code,
                stderr,
                ..
            } => {
                assert_eq!(*code, Some(3));
                assert!(stderr.contains("No module named"), "{stderr}");
            }
            other => panic!("unexpected error {other:?}"),
        }
        assert_eq!(daemon.session_count(), 0);
    }

    /// Environment for a kernel whose first `failures` starts lose a port,
    /// counting its spawns in `count_file`.
    fn failing_first(
        kernel: &FakeKernel,
        failures: usize,
        count_file: &Path,
    ) -> Vec<(String, String)> {
        let mut env = kernel.env.clone();
        env.push((
            FAIL_FIRST_ENV.to_string(),
            format!("{failures}:{}", count_file.display()),
        ));
        env
    }

    fn spawns(count_file: &Path) -> usize {
        std::fs::read_to_string(count_file).map_or(0, |s| s.lines().count())
    }

    /// bd-e87d5tmn: a kernel that loses a port it was handed (another socket
    /// took it between the daemon peeking it and the kernel binding it) is
    /// started again on fresh ports, and the document gets a working kernel.
    #[tokio::test]
    async fn test_kernel_losing_a_port_at_startup_is_retried() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &pid_script());
        let count = home.jupyter_path().join("spawns");
        let env = failing_first(&kernel, 1, &count);
        let cwd = std::env::current_dir().unwrap();
        let daemon = JupyterDaemon::new();

        let mut lease = daemon
            .acquire(&kernel.name, &cwd, &cwd.join("a.qmd"), &env)
            .await
            .expect("the retry starts a kernel");
        assert!(!pid_of(&mut lease).await.trim().is_empty());
        assert_eq!(spawns(&count), 2);
        daemon.shutdown_lease(lease).await.unwrap();
        assert_eq!(daemon.session_count(), 0);
    }

    /// The retry is bounded: a kernel that loses a port on every attempt fails
    /// the start with the kernel's own report.
    #[tokio::test]
    async fn test_kernel_always_losing_a_port_fails_after_bounded_retries() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &pid_script());
        let count = home.jupyter_path().join("spawns");
        let env = failing_first(&kernel, usize::MAX, &count);
        let cwd = std::env::current_dir().unwrap();
        let daemon = JupyterDaemon::new();

        let err = match daemon
            .acquire(&kernel.name, &cwd, &cwd.join("a.qmd"), &env)
            .await
        {
            Ok(_) => panic!("a kernel that never binds cannot be leased"),
            Err(e) => e,
        };
        assert!(err.is_port_conflict(), "{err:?}");
        assert_eq!(spawns(&count), 3);
        assert_eq!(daemon.session_count(), 0);
    }

    /// bd-0w8rgjdf: a fake kernel that cannot bind a port it was handed exits
    /// non-zero with the bind error on stderr, as ipykernel does, so the daemon
    /// sees a port conflict and retries. It used to discard the error and exit
    /// 0 with empty stderr, which the daemon could not classify (a CI flake).
    #[tokio::test]
    async fn test_fake_kernel_losing_a_port_reports_a_port_conflict() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("hold a port");
        let free: Vec<u16> = (0..4)
            .map(|_| std::net::TcpListener::bind("127.0.0.1:0").unwrap())
            .collect::<Vec<_>>()
            .iter()
            .map(|l| l.local_addr().unwrap().port())
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let connection_file = dir.path().join("connection.json");
        let connection = serde_json::json!({
            "ip": "127.0.0.1",
            "transport": "tcp",
            "shell_port": taken.local_addr().unwrap().port(),
            "iopub_port": free[0],
            "stdin_port": free[1],
            "control_port": free[2],
            "hb_port": free[3],
            "signature_scheme": "hmac-sha256",
            "key": "k",
            "kernel_name": "fake-py",
        });
        std::fs::write(&connection_file, connection.to_string()).unwrap();

        let output = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", ENTRY_TEST, "--nocapture"])
                .arg(&connection_file)
                .env(SCRIPT_ENV, "{}")
                .env_remove(SCRIPTS_ENV)
                .env_remove(EXIT_ENV)
                .env_remove(FAIL_FIRST_ENV)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("a kernel that cannot bind exits")
        .unwrap();
        drop(taken);

        let err = crate::engine::jupyter::error::JupyterError::ExitedDuringStartup {
            kernel: "fake-py".to_string(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };
        assert_ne!(output.status.code(), Some(0), "{err:?}");
        assert!(err.is_port_conflict(), "{err:?}");
    }

    /// A kernel that exits for any other reason is not retried.
    #[tokio::test]
    async fn test_kernel_exiting_for_another_reason_is_not_retried() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &pid_script());
        let count = home.jupyter_path().join("spawns");
        // A fail-first of 0 only counts the spawns; EXIT_ENV makes it exit.
        let mut env = failing_first(&kernel, 0, &count);
        env.push((EXIT_ENV.to_string(), "SyntaxError: boom".to_string()));
        let cwd = std::env::current_dir().unwrap();
        let daemon = JupyterDaemon::new();

        let err = match daemon
            .acquire(&kernel.name, &cwd, &cwd.join("a.qmd"), &env)
            .await
        {
            Ok(_) => panic!("a kernel that exits cannot be leased"),
            Err(e) => e,
        };
        assert!(!err.is_port_conflict(), "{err:?}");
        assert_eq!(spawns(&count), 1);
    }
}
