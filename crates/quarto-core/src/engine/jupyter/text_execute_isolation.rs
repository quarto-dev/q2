/*
 * engine/jupyter/text_execute_isolation.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-i7yz08uh: every document runs on its own kernel.
 */

//! Per-document kernel isolation (bd-i7yz08uh), through the real `execute_qmd`, the
//! global daemon and the fake kernel.
//!
//! Before the fix, the daemon keyed kernels on (kernel name, working directory) and
//! the working directory is the project directory, so every document of a project
//! shared one kernel: a parallel render interleaved two documents' cells on it, and
//! even a serial render let a document read another's globals. Quarto 1 never shares
//! a kernel between documents, and neither does q2 now.
//!
//! The fake kernel keeps no state, so these tests observe *which kernel process* ran
//! a cell ([`FakeOutput::KernelPid`]) rather than a leaked variable; the real-kernel
//! counterparts (interleaving, leaked globals) drive the `q2` binary in
//! `crates/quarto/tests/integration/jupyter_document_isolation_e2e.rs`.

use std::path::Path;

use super::span_tests::{ctx_for, jupyter_meta};
use super::*;
use crate::engine::jupyter::daemon::{daemon, kernel_scope, kernel_scope_retaining};
use crate::engine::jupyter::fake_kernel::{FakeKernel, FakeKernelHome, FakeOutput, Script};

const DOC: &str = "```{python}\npid\n```\n";

/// One fake python kernelspec whose `pid` cell prints the kernel's process id.
fn install() -> (FakeKernelHome, FakeKernel) {
    let home = FakeKernelHome::new();
    let script = Script::from([("pid\n".to_string(), vec![FakeOutput::KernelPid])]);
    let kernel = home.install("fake-py", "python", &script);
    (home, kernel)
}

/// Execute [`DOC`] as the document `source` and return the pid of the kernel that ran
/// its cell.
fn kernel_pid_for(kernel: &FakeKernel, source: &Path) -> u32 {
    let mut ctx = ctx_for(DOC)
        .with_metadata(jupyter_meta(&kernel.name))
        .with_owned_languages(vec!["python".to_string()])
        .with_project_env(kernel.env.clone());
    ctx.source_path = source.to_path_buf();
    let markdown = execute_qmd(DOC, &ctx).expect("execution succeeds").markdown;
    let pids: Vec<u32> = markdown
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();
    match pids.as_slice() {
        [pid] => *pid,
        other => panic!("expected one kernel pid in the output, got {other:?}:\n{markdown}"),
    }
}

fn session_count() -> usize {
    daemon().session_count()
}

/// Two documents in one directory (one working directory, one kernelspec) run on two
/// kernels. Before the fix they shared one, which is what let cells interleave.
#[test]
fn two_documents_in_one_directory_run_on_different_kernels() {
    let (_home, kernel) = install();
    let dir = tempfile::tempdir().unwrap();
    // A render-wide scope, as `q2 render` holds one for the whole project.
    let _scope = kernel_scope();

    let a = kernel_pid_for(&kernel, &dir.path().join("a.qmd"));
    let b = kernel_pid_for(&kernel, &dir.path().join("b.qmd"));
    assert_ne!(a, b, "a.qmd and b.qmd ran on the same kernel process");
}

/// Under a releasing scope (`q2 render`), a document's kernel is shut down as soon as
/// the document finishes, not when the render ends.
#[test]
fn a_document_kernel_is_shut_down_when_the_document_finishes() {
    let (_home, kernel) = install();
    let dir = tempfile::tempdir().unwrap();
    let _scope = kernel_scope();

    kernel_pid_for(&kernel, &dir.path().join("a.qmd"));
    assert_eq!(
        session_count(),
        0,
        "a.qmd's kernel outlived the document while the render scope is still open"
    );
}

/// Under a retaining scope (`q2 preview`), re-rendering a document reuses its warm
/// kernel, while another document still gets its own.
#[test]
fn a_retaining_scope_keeps_each_document_kernel_warm() {
    let (_home, kernel) = install();
    let dir = tempfile::tempdir().unwrap();
    let _scope = kernel_scope_retaining();

    let a1 = kernel_pid_for(&kernel, &dir.path().join("a.qmd"));
    let a2 = kernel_pid_for(&kernel, &dir.path().join("a.qmd"));
    let b = kernel_pid_for(&kernel, &dir.path().join("b.qmd"));
    assert_eq!(a1, a2, "re-rendering a.qmd started a new kernel");
    assert_ne!(a1, b, "b.qmd reused a.qmd's kernel");
    assert_eq!(session_count(), 2);
}
