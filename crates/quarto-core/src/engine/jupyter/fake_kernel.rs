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
//! child through `get_or_start_session`'s `extra_env`. A kernelspec directory is
//! written under a temp dir that `JUPYTER_PATH` points at, so
//! `find_kernelspec` discovers it through the real code path. Setting the variable is
//! process-global; nextest runs each test in its own process, which is what makes
//! that safe (the same precedent as `extension/mod.rs`).
//!
//! JOL-5 extends this (a second kernel name, file writes); EXR-5 reuses it.

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
}

/// Code (exact match) to the outputs the kernel sends for it. Code not in the script
/// is echoed back as stdout, by `TestKernel`'s default.
pub(crate) type Script = BTreeMap<String, Vec<FakeOutput>>;

impl FakeOutput {
    /// `execute_result` with a single `text/plain` value.
    pub(crate) fn plain_result(text: &str) -> Self {
        Self::Result(serde_json::json!({ "text/plain": text }))
    }

    fn into_message(self) -> JupyterMessageContent {
        match self {
            Self::Stdout(text) => StreamContent::stdout(&text).into(),
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
}

/// An installed fake kernel: its kernelspec name and the environment to pass to
/// `get_or_start_session` so the spawned process runs the scripted kernel.
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
        Self { dir }
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
        FakeKernel {
            name: name.to_string(),
            env: vec![(
                SCRIPT_ENV.to_string(),
                serde_json::to_string(script).expect("script json"),
            )],
        }
    }

    #[allow(dead_code)] // for JOL-5's tests that inspect the directory
    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }
}

/// Run the fake kernel on the connection file named by the last command-line argument
/// until the process is killed.
fn run_kernel_process(script_json: &str) {
    let script: Script = serde_json::from_str(script_json).expect("script json");
    let connection_file = std::env::args()
        .next_back()
        .expect("connection file argument");
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
        if let Ok(script_json) = std::env::var(SCRIPT_ENV) {
            run_kernel_process(&script_json);
        }
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
    async fn test_fake_kernel_echo_round_trip() {
        let home = FakeKernelHome::new();
        let kernel = home.install("fake-py", "python", &echo_script());
        let cwd = std::env::current_dir().unwrap();
        let daemon = JupyterDaemon::new();

        let key = daemon
            .get_or_start_session(&kernel.name, &cwd, &kernel.env)
            .await
            .expect("fake kernel starts through the real session path");

        // Scripted: one stream, then the value.
        let r = daemon
            .execute_in_session(&key, "echo hi")
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            r.outputs.as_slice(),
            [CellOutput::Stream { name, text }, CellOutput::ExecuteResult { data, .. }]
                if name == "stdout" && text == "hi\n"
                    && data.get("text/plain") == Some(&serde_json::json!("'hi'"))
        ));

        // A silent execution (statement, or a `None` value): no outputs at all.
        let r = daemon
            .execute_in_session(&key, "x = 1")
            .await
            .unwrap()
            .unwrap();
        assert!(r.outputs.is_empty(), "got {:?}", r.outputs);

        // An error message.
        let r = daemon
            .execute_in_session(&key, "boom")
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            r.outputs.as_slice(),
            [CellOutput::Error { ename, .. }] if ename == "NameError"
        ));

        // Unscripted code is echoed as stdout (TestKernel's default).
        let r = daemon
            .execute_in_session(&key, "anything else")
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            r.outputs.as_slice(),
            [CellOutput::Stream { text, .. }] if text == "anything else"
        ));

        daemon.shutdown_all().await.unwrap();
    }
}
