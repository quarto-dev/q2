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
            let _ = handle.await;
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
        if let Ok(message) = std::env::var(EXIT_ENV) {
            eprintln!("{message}");
            std::process::exit(3);
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
            "zmq.error.ZMQError: Address already in use".to_string(),
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
                assert!(stderr.contains("Address already in use"), "{stderr}");
            }
            other => panic!("unexpected error {other:?}"),
        }
        assert_eq!(daemon.session_count(), 0);
    }
}
