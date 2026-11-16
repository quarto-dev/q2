/*
 * tests/integration/terminal_output_style.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end pins for q2's terminal output style (bd-be8ve52f, GH #1135):
 * `--color`, `NO_COLOR`, `CLICOLOR_FORCE` / `FORCE_COLOR` and
 * `FORCE_HYPERLINK` decide whether stderr carries ANSI color (SGR) and
 * OSC 8 hyperlinks. The precedence table itself is unit-tested in
 * `quarto_util::output_style`; these tests check that the real binary
 * applies it to every stderr sink a render uses.
 *
 * Every run captures stderr, so stderr is never a terminal here: the
 * "auto" answer is always plain, and color appears only when forced.
 */

use std::process::Command;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// The GH #1135 fixture: an include of a file that does not exist, so
/// the render prints an error diagnostic with a source snippet and a
/// file link (162 SGR + 2 OSC 8 sequences before the fix).
const MISSING_INCLUDE: &str = "---\ntitle: Hello\n---\n\n{{< include missing.qmd >}}\n";

/// A document that renders cleanly, for the `-vv` tracing output.
const CLEAN: &str = "---\ntitle: ok\n---\n\nhi\n";

/// Variables that steer the decision; removed from every run so the
/// developer's or CI's environment cannot leak into a test.
const STYLE_VARS: &[&str] = &[
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "FORCE_COLOR",
    "FORCE_HYPERLINK",
    "CI",
    "RUST_LOG",
];

struct Escapes {
    sgr: usize,
    osc8: usize,
    stderr: String,
}

impl std::fmt::Debug for Escapes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} SGR, {} OSC 8; stderr:\n{}",
            self.sgr, self.osc8, self.stderr
        )
    }
}

/// Render `doc` (as `index.qmd` in a fresh directory) with `args` and
/// `env`, and count the escapes on stderr.
fn run(doc: &str, args: &[&str], env: &[(&str, &str)]) -> Escapes {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("index.qmd"), doc).unwrap();
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(dir.path()).env("TERM", "xterm-256color");
    for var in STYLE_VARS {
        cmd.env_remove(var);
    }
    cmd.envs(env.iter().copied());
    let output = cmd.args(args).output().expect("q2 runs");
    count(&output.stderr)
}

fn render(doc: &str, extra: &[&str], env: &[(&str, &str)]) -> Escapes {
    let mut args = vec!["render", "index.qmd"];
    args.extend_from_slice(extra);
    run(doc, &args, env)
}

fn count(bytes: &[u8]) -> Escapes {
    let stderr = String::from_utf8_lossy(bytes).to_string();
    Escapes {
        sgr: stderr.matches("\x1b[").count(),
        osc8: stderr.matches("\x1b]8;").count(),
        stderr,
    }
}

// ── diagnostics ──────────────────────────────────────────────────────

/// GH #1135: stderr redirected to a file gets no escapes at all.
#[test]
fn diagnostics_to_a_pipe_are_plain() {
    let e = render(MISSING_INCLUDE, &[], &[]);
    assert!(e.stderr.contains("Q-17-2"), "{e:?}");
    assert_eq!((e.sgr, e.osc8), (0, 0), "{e:?}");
}

#[test]
fn no_color_beats_clicolor_force() {
    let e = render(
        MISSING_INCLUDE,
        &[],
        &[("NO_COLOR", "1"), ("CLICOLOR_FORCE", "1")],
    );
    assert_eq!((e.sgr, e.osc8), (0, 0), "{e:?}");
}

#[test]
fn clicolor_force_colors_a_pipe_without_hyperlinks() {
    let e = render(MISSING_INCLUDE, &[], &[("CLICOLOR_FORCE", "1")]);
    assert!(e.sgr > 0, "{e:?}");
    assert_eq!(e.osc8, 0, "{e:?}");
}

#[test]
fn color_always_colors_a_pipe_without_hyperlinks() {
    let e = render(
        MISSING_INCLUDE,
        &["--color", "always"],
        &[("NO_COLOR", "1")],
    );
    assert!(e.sgr > 0, "{e:?}");
    assert_eq!(e.osc8, 0, "{e:?}");
}

#[test]
fn color_never_beats_force_variables() {
    let e = render(
        MISSING_INCLUDE,
        &["--color=never"],
        &[("CLICOLOR_FORCE", "1"), ("FORCE_HYPERLINK", "1")],
    );
    assert_eq!((e.sgr, e.osc8), (0, 0), "{e:?}");
}

#[test]
fn force_hyperlink_adds_links() {
    let e = render(
        MISSING_INCLUDE,
        &["--color=always"],
        &[("FORCE_HYPERLINK", "1")],
    );
    assert!(e.sgr > 0, "{e:?}");
    assert!(e.osc8 > 0, "{e:?}");
}

/// `--color` is a global flag: accepted before the subcommand too.
#[test]
fn color_flag_is_global() {
    let e = run(
        MISSING_INCLUDE,
        &["--color", "always", "render", "index.qmd"],
        &[],
    );
    assert!(e.sgr > 0, "{e:?}");
}

// ── tracing (`-v`) ───────────────────────────────────────────────────

#[test]
fn tracing_to_a_pipe_is_plain() {
    let e = render(CLEAN, &["-vv"], &[]);
    assert!(e.stderr.contains("DEBUG"), "{e:?}");
    assert_eq!(e.sgr, 0, "{e:?}");
}

#[test]
fn tracing_follows_color_always() {
    let e = render(CLEAN, &["-vv", "--color=always"], &[]);
    assert!(e.sgr > 0, "{e:?}");
}

// ── render aborts (structured parse errors printed by the CLI) ───────
//
// These paths printed `ParseError`'s `Display`, which baked in color and
// hyperlinks regardless of the stream. Each gets a pipe test (plain) and
// a forced-color guard, so making `Display` plain can't silently drop
// color where the user asked for it.

/// A project whose `_quarto.yml` is `config`, with one page, rendered
/// with `args` (after `render`).
fn render_project(config: &str, args: &[&str], env: &[(&str, &str)]) -> Escapes {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("_quarto.yml"), config).unwrap();
    std::fs::write(dir.path().join("index.qmd"), CLEAN).unwrap();
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(dir.path()).env("TERM", "xterm-256color");
    for var in STYLE_VARS {
        cmd.env_remove(var);
    }
    cmd.envs(env.iter().copied());
    let output = cmd.arg("render").args(args).output().expect("q2 runs");
    assert!(
        !output.status.success(),
        "the fixture must abort the render"
    );
    count(&output.stderr)
}

/// Discovery fails on a YAML syntax error: `RenderAbort::Dispatch`.
const BROKEN_CONFIG: &str = "project:\n  type: website\n  title: [unclosed\n";

/// A pre-render script that does not exist: `RenderAbort::Scripts`.
const MISSING_PRE_RENDER: &str = "project:\n  type: website\n  pre-render: nothere.py\n";

#[test]
fn config_abort_to_a_pipe_is_plain() {
    for args in [&[][..], &["index.qmd"][..]] {
        let e = render_project(BROKEN_CONFIG, args, &[]);
        assert!(e.stderr.contains("Q-1-1"), "{args:?}: {e:?}");
        assert_eq!((e.sgr, e.osc8), (0, 0), "{args:?}: {e:?}");
    }
}

#[test]
fn config_abort_keeps_color_when_forced() {
    let e = render_project(BROKEN_CONFIG, &["--color=always"], &[]);
    assert!(e.stderr.contains("Q-1-1"), "{e:?}");
    assert!(e.sgr > 0, "{e:?}");
}

#[test]
fn script_abort_to_a_pipe_is_plain() {
    let e = render_project(MISSING_PRE_RENDER, &[], &[]);
    assert!(e.stderr.contains("nothere.py"), "{e:?}");
    assert_eq!((e.sgr, e.osc8), (0, 0), "{e:?}");
}

#[test]
fn script_abort_keeps_color_when_forced() {
    let e = render_project(MISSING_PRE_RENDER, &["--color=always"], &[]);
    assert!(e.sgr > 0, "{e:?}");
}
