//! Terminal output style: whether q2 decorates a stream with ANSI color
//! and OSC 8 hyperlinks.
//!
//! One policy for every binary and every sink (bd-be8ve52f, GH #1135).
//! The decision is made **once**, on the main thread, first thing after
//! argument parsing ([`init`]), and read back with [`get`]. Formatting
//! code takes the resolved [`StreamStyle`] explicitly (like the `quiet`
//! flag of [`crate::user_status!`]) so it stays testable without global
//! state; only command entry points call [`get`].
//!
//! # Color, per stream (first match wins)
//!
//! 1. `--color=never` → off; `--color=always` → on.
//! 2. `NO_COLOR` set to a non-empty value → off (<https://no-color.org>).
//! 3. `CLICOLOR_FORCE` non-empty, or `FORCE_COLOR` non-empty and not
//!    `0` → on.
//! 4. `CLICOLOR=0` → off.
//! 5. The stream is a terminal and (`TERM` supports color, `CLICOLOR` is
//!    set, or `CI` is set) → on.
//! 6. Otherwise off.
//!
//! Rules 2–6 are the precedence `anstream` 1.0 uses for `ColorChoice::Auto`
//! (`anstream::auto::choice`), so `q2 --help` (clap) and the rest of q2
//! agree. They are restated here, rather than called through
//! `anstyle-query`, because that crate reads the process environment and
//! the resolver must take its environment as an input to be testable.
//!
//! # Hyperlinks, per stream
//!
//! - `--color=never` → off.
//! - `FORCE_HYPERLINK` set → on, unless its value is `0`.
//! - Otherwise on only when color is on, the stream is a terminal, and the
//!   terminal is known to render OSC 8 (the `supports-hyperlinks`
//!   heuristic). Forcing color into a pipe or a CI log therefore never
//!   adds hyperlinks — CI log viewers show OSC 8 as noise.

use std::ffi::OsString;
use std::sync::OnceLock;

/// The `--color` command-line choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorFlag {
    /// Decide from the environment and whether the stream is a terminal.
    #[default]
    Auto,
    /// Color on every stream, even a pipe. Hyperlinks still follow their
    /// own rule.
    Always,
    /// No color and no hyperlinks.
    Never,
}

/// How one output stream is decorated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamStyle {
    /// Emit ANSI SGR color sequences.
    pub color: bool,
    /// Emit OSC 8 hyperlinks.
    pub hyperlinks: bool,
}

impl StreamStyle {
    /// No decoration at all.
    pub const PLAIN: StreamStyle = StreamStyle {
        color: false,
        hyperlinks: false,
    };
}

/// The resolved style of the process's two output streams.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputStyle {
    pub stdout: StreamStyle,
    pub stderr: StreamStyle,
}

impl OutputStyle {
    /// No decoration on either stream.
    pub const PLAIN: OutputStyle = OutputStyle {
        stdout: StreamStyle::PLAIN,
        stderr: StreamStyle::PLAIN,
    };
}

/// Everything the decision reads, so [`resolve`] is a pure function.
pub struct StyleInputs<'a> {
    pub flag: ColorFlag,
    /// Environment lookup (`std::env::var_os` in production).
    pub env: &'a dyn Fn(&str) -> Option<OsString>,
    pub stdout_is_terminal: bool,
    pub stderr_is_terminal: bool,
    /// Whether the terminal is known to render OSC 8 hyperlinks
    /// (`supports_hyperlinks::supports_hyperlinks()` in production).
    pub terminal_supports_hyperlinks: bool,
}

/// Resolve the output style from its inputs. Pure: no process state.
pub fn resolve(inputs: &StyleInputs) -> OutputStyle {
    OutputStyle {
        stdout: resolve_stream(inputs, inputs.stdout_is_terminal),
        stderr: resolve_stream(inputs, inputs.stderr_is_terminal),
    }
}

fn resolve_stream(inputs: &StyleInputs, is_terminal: bool) -> StreamStyle {
    let env = inputs.env;
    let color = match inputs.flag {
        ColorFlag::Never => return StreamStyle::PLAIN,
        ColorFlag::Always => true,
        ColorFlag::Auto => auto_color(env, is_terminal),
    };
    let hyperlinks = match env("FORCE_HYPERLINK") {
        // `supports-hyperlinks` semantics: set means forced, unless `0`.
        Some(v) => v.to_string_lossy().trim() != "0",
        None => color && is_terminal && inputs.terminal_supports_hyperlinks,
    };
    StreamStyle { color, hyperlinks }
}

/// Rules 2–6 of the module docs (`anstream::auto::choice`).
fn auto_color(env: &dyn Fn(&str) -> Option<OsString>, is_terminal: bool) -> bool {
    let non_empty = |name: &str| env(name).is_some_and(|v| !v.is_empty());
    let clicolor = env("CLICOLOR").map(|v| v != "0");
    if non_empty("NO_COLOR") {
        false
    } else if non_empty("CLICOLOR_FORCE")
        || env("FORCE_COLOR").is_some_and(|v| !v.is_empty() && v != "0")
    {
        true
    } else if clicolor == Some(false) {
        false
    } else {
        is_terminal && (term_supports_color(env) || clicolor == Some(true) || env("CI").is_some())
    }
}

/// `anstyle_query::term_supports_color`, over an injected environment.
fn term_supports_color(env: &dyn Fn(&str) -> Option<OsString>) -> bool {
    match env("TERM") {
        Some(term) => term != "dumb",
        // Unix terminals set TERM; an unset TERM means an odd environment.
        // Windows consoles commonly leave it unset and do support color.
        None => cfg!(windows),
    }
}

static STYLE: OnceLock<OutputStyle> = OnceLock::new();

/// Resolve the style from the real process (environment, terminals) and
/// record it for [`get`]. Call it on the main thread, before starting any
/// runtime or thread and before printing anything. A second call keeps
/// the first result and returns it.
pub fn init(flag: ColorFlag) -> OutputStyle {
    *STYLE.get_or_init(|| {
        use std::io::IsTerminal;
        let env = |name: &str| std::env::var_os(name);
        let style = resolve(&StyleInputs {
            flag,
            env: &env,
            stdout_is_terminal: std::io::stdout().is_terminal(),
            stderr_is_terminal: std::io::stderr().is_terminal(),
            terminal_supports_hyperlinks: supports_hyperlinks::supports_hyperlinks(),
        });
        // Windows consoles interpret ANSI only once virtual-terminal
        // processing is enabled; a no-op elsewhere.
        if (style.stdout.color && std::io::stdout().is_terminal())
            || (style.stderr.color && std::io::stderr().is_terminal())
        {
            let _ = anstyle_query::windows::enable_ansi_colors();
        }
        style
    })
}

/// The style recorded by [`init`], or [`OutputStyle::PLAIN`] when `init`
/// never ran (a library caller, a test, WASM): undecorated output is the
/// safe default.
pub fn get() -> OutputStyle {
    STYLE.get().copied().unwrap_or(OutputStyle::PLAIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Resolve with `vars` as the whole environment.
    fn style(
        flag: ColorFlag,
        vars: &[(&str, &str)],
        stdout_tty: bool,
        stderr_tty: bool,
        links: bool,
    ) -> OutputStyle {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        let env = move |k: &str| map.get(k).cloned();
        resolve(&StyleInputs {
            flag,
            env: &env,
            stdout_is_terminal: stdout_tty,
            stderr_is_terminal: stderr_tty,
            terminal_supports_hyperlinks: links,
        })
    }

    /// stderr's style with stdout a pipe.
    fn err(flag: ColorFlag, vars: &[(&str, &str)], tty: bool, links: bool) -> StreamStyle {
        style(flag, vars, false, tty, links).stderr
    }

    const TERM: (&str, &str) = ("TERM", "xterm-256color");
    const COLOR: StreamStyle = StreamStyle {
        color: true,
        hyperlinks: false,
    };
    const FULL: StreamStyle = StreamStyle {
        color: true,
        hyperlinks: true,
    };
    const PLAIN: StreamStyle = StreamStyle::PLAIN;
    use ColorFlag::{Always, Auto, Never};

    // ── rule 5/6: terminal detection ─────────────────────────────────

    #[test]
    fn auto_colors_a_capable_terminal() {
        assert_eq!(err(Auto, &[TERM], true, true), FULL);
        assert_eq!(err(Auto, &[TERM], true, false), COLOR);
    }

    #[test]
    fn auto_leaves_a_pipe_plain() {
        // GH #1135: stderr redirected to a log file.
        assert_eq!(err(Auto, &[TERM], false, true), PLAIN);
    }

    #[test]
    fn auto_leaves_a_dumb_terminal_plain() {
        assert_eq!(err(Auto, &[("TERM", "dumb")], true, true), PLAIN);
    }

    #[cfg(not(windows))]
    #[test]
    fn auto_without_term_is_plain_on_unix() {
        assert_eq!(err(Auto, &[], true, true), PLAIN);
    }

    #[cfg(windows)]
    #[test]
    fn auto_without_term_colors_on_windows() {
        assert_eq!(err(Auto, &[], true, true), FULL);
    }

    #[test]
    fn clicolor_or_ci_rescue_a_terminal_without_term() {
        assert_eq!(
            err(Auto, &[("TERM", "dumb"), ("CLICOLOR", "1")], true, false),
            COLOR
        );
        assert_eq!(
            err(Auto, &[("TERM", "dumb"), ("CI", "true")], true, false),
            COLOR
        );
        // ...but not a pipe.
        assert_eq!(err(Auto, &[("CLICOLOR", "1")], false, false), PLAIN);
    }

    #[test]
    fn streams_are_decided_independently() {
        let s = style(Auto, &[TERM], true, false, true);
        assert_eq!(s.stdout, FULL);
        assert_eq!(s.stderr, PLAIN);
    }

    // ── rule 2: NO_COLOR ─────────────────────────────────────────────

    #[test]
    fn no_color_turns_everything_off() {
        assert_eq!(err(Auto, &[TERM, ("NO_COLOR", "1")], true, true), PLAIN);
    }

    #[test]
    fn empty_no_color_is_unset() {
        assert_eq!(err(Auto, &[TERM, ("NO_COLOR", "")], true, true), FULL);
    }

    #[test]
    fn no_color_beats_force_variables() {
        for force in [("CLICOLOR_FORCE", "1"), ("FORCE_COLOR", "1")] {
            assert_eq!(
                err(Auto, &[TERM, ("NO_COLOR", "1"), force], false, true),
                PLAIN
            );
        }
    }

    // ── rule 3: forcing ──────────────────────────────────────────────

    #[test]
    fn force_variables_color_a_pipe_without_hyperlinks() {
        for force in [
            ("CLICOLOR_FORCE", "1"),
            ("FORCE_COLOR", "1"),
            ("FORCE_COLOR", "3"),
        ] {
            assert_eq!(err(Auto, &[TERM, force], false, true), COLOR, "{force:?}");
        }
    }

    #[test]
    fn empty_or_zero_force_does_not_force() {
        for force in [
            ("CLICOLOR_FORCE", ""),
            ("FORCE_COLOR", ""),
            ("FORCE_COLOR", "0"),
        ] {
            assert_eq!(err(Auto, &[TERM, force], false, true), PLAIN, "{force:?}");
        }
    }

    #[test]
    fn force_beats_clicolor_zero() {
        assert_eq!(
            err(
                Auto,
                &[TERM, ("CLICOLOR", "0"), ("CLICOLOR_FORCE", "1")],
                false,
                false
            ),
            COLOR
        );
    }

    // ── rule 4: CLICOLOR=0 ───────────────────────────────────────────

    #[test]
    fn clicolor_zero_turns_a_terminal_off() {
        assert_eq!(err(Auto, &[TERM, ("CLICOLOR", "0")], true, true), PLAIN);
    }

    // ── rule 1: the flag ─────────────────────────────────────────────

    #[test]
    fn color_always_beats_no_color_and_a_pipe() {
        assert_eq!(err(Always, &[TERM, ("NO_COLOR", "1")], false, true), COLOR);
    }

    #[test]
    fn color_always_on_a_capable_terminal_keeps_hyperlinks() {
        assert_eq!(err(Always, &[TERM], true, true), FULL);
    }

    #[test]
    fn color_never_beats_everything() {
        let vars = [TERM, ("CLICOLOR_FORCE", "1"), ("FORCE_HYPERLINK", "1")];
        assert_eq!(err(Never, &vars, true, true), PLAIN);
    }

    // ── hyperlinks ───────────────────────────────────────────────────

    #[test]
    fn hyperlinks_need_a_capable_terminal() {
        assert_eq!(err(Auto, &[TERM], true, false), COLOR);
    }

    #[test]
    fn force_hyperlink_forces_links_even_in_a_pipe() {
        let s = err(Always, &[TERM, ("FORCE_HYPERLINK", "1")], false, false);
        assert_eq!(s, FULL);
        // Independent of color: an explicit request is honored.
        let s = err(Auto, &[TERM, ("FORCE_HYPERLINK", "1")], false, false);
        assert_eq!(
            s,
            StreamStyle {
                color: false,
                hyperlinks: true
            }
        );
    }

    #[test]
    fn force_hyperlink_zero_disables_links() {
        assert_eq!(
            err(Auto, &[TERM, ("FORCE_HYPERLINK", "0")], true, true),
            COLOR
        );
    }

    // ── the global ───────────────────────────────────────────────────

    /// No test in this binary calls `init`, so `get` must see nothing
    /// recorded and fail safe.
    #[test]
    fn get_before_init_is_plain() {
        assert_eq!(get(), OutputStyle::PLAIN);
    }
}
