//! bd-i7yz08uh: documents of a project never share Jupyter kernel state.
//!
//! Before the fix, the kernel daemon keyed sessions on (kernel name, working
//! directory), and the working directory of a project render is the project
//! directory, so every python document of a project ran on one kernel:
//!
//! - a parallel render (`QUARTO_JOBS=2`) interleaved two documents' cells on
//!   that kernel, so a document could read a value another document had just
//!   set (`doc a sees owner = b`);
//! - even a serial render (`QUARTO_JOBS=1`) let a document read the globals of
//!   whichever document had run before it.
//!
//! Quarto 1 never shares a kernel between documents (a fresh kernel per
//! document, or a keepalive kernel keyed by the document's path), and q2 now
//! gives each document its own kernel.
//!
//! These drive the real `q2` binary against a real `python3` kernel, gated
//! like the other Jupyter e2e tests. A skip is an environment signal, not a
//! pass. The kernel-free counterparts are the fake-kernel tests in
//! `crates/quarto-core/src/engine/jupyter/text_execute_isolation.rs`.

use std::path::Path;
use std::process::Command;

use crate::jupyter_kernel_cleanup_e2e::{jupyter_available, python3_kernel_available};

fn python3_ready() -> bool {
    if !jupyter_available() {
        eprintln!("skipping: jupyter not available");
        return false;
    }
    python3_kernel_available()
}

/// A `type: default` project at `dir/proj` holding `docs` (name, body).
fn project(dir: &Path, docs: &[(&str, String)]) -> std::path::PathBuf {
    let project = dir.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    for (name, body) in docs {
        std::fs::write(
            project.join(format!("{name}.qmd")),
            format!("---\ntitle: {name}\nengine: jupyter\n---\n\n{body}"),
        )
        .unwrap();
    }
    project
}

fn render(project: &Path, jobs: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_q2"))
        .arg("render")
        .arg(project)
        .env("QUARTO_JOBS", jobs)
        .output()
        .expect("q2 render runs");
    assert!(
        output.status.success(),
        "q2 render failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn html(project: &Path, name: &str) -> String {
    std::fs::read_to_string(project.join(format!("{name}.html")))
        .unwrap_or_else(|e| panic!("read {name}.html: {e}"))
}

/// Two documents render in parallel. Each sets `owner`, pauses, and reads
/// `owner` back.
///
/// Why four cells: on a shared kernel the old daemon serialized requests per
/// cell through a fair lock, and a document has at most one request waiting,
/// so two contending documents strictly alternate. The other document's
/// `owner = ...` cell therefore lands right after the cell in which it
/// arrived. Arriving during the sleep (or earlier) puts it before the
/// filler cell and so before the read. A two-cell document (set; sleep+read)
/// only interleaves when the other document arrives in the narrow gap
/// between the two cells.
#[test]
fn parallel_documents_do_not_interleave_on_a_kernel() {
    if !python3_ready() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let doc = |name: &str| {
        format!(
            "```{{python}}\nowner = \"{name}\"\n```\n\n\
             ```{{python}}\nimport time\ntime.sleep(2)\n```\n\n\
             ```{{python}}\npass\n```\n\n\
             ```{{python}}\nprint(\"doc {name} sees owner =\", owner)\n```\n"
        )
    };
    let project = project(tmp.path(), &[("a", doc("a")), ("b", doc("b"))]);

    render(&project, "2");

    for name in ["a", "b"] {
        let page = html(&project, name);
        assert!(
            page.contains(&format!("doc {name} sees owner = {name}")),
            "{name}.html shows another document's `owner`:\n{page}"
        );
    }
}

/// A serial render: each document reports whether the other document's global
/// is visible, then defines its own. Whichever runs second would see the
/// first one's global on a shared kernel.
#[test]
fn documents_do_not_see_each_others_globals() {
    if !python3_ready() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let doc = |name: &str, other: &str| {
        format!(
            "```{{python}}\nprint(\"{name} sees {other}:\", \
             globals().get(\"defined_by_{other}\", \"nothing\"))\n\
             defined_by_{name} = True\n```\n"
        )
    };
    let project = project(tmp.path(), &[("a", doc("a", "b")), ("b", doc("b", "a"))]);

    render(&project, "1");

    for (name, other) in [("a", "b"), ("b", "a")] {
        let page = html(&project, name);
        assert!(
            page.contains(&format!("{name} sees {other}: nothing")),
            "{name}.html sees a global {other}.qmd defined:\n{page}"
        );
    }
}
