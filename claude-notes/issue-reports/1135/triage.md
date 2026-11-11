# Issue #1135: `q2 render` ignores `NO_COLOR` in its diagnostics

- **GitHub**: https://github.com/quarto-dev/q2/issues/1135
- **Reporter**: mcanouil (Mickaël Canouil), 2026-11-09
- **Triage date**: 2026-11-11
- **Worktree**: `.worktrees/issue-1135` (branch `issue-1135`, based on `main` @ `e14065e79`)
- **Braid strand**: bd-be8ve52f (existing, filed 2026-10-09; this issue duplicates it)
- **Scope**: the reported bug (the render diagnostics ignore `NO_COLOR` and
  non-tty stderr). The maintainer also asked whether to use the bug to
  consolidate how q2 decides how much styling its terminal output gets. This
  doc covers both. Interactive-prompt gating (`stdin().is_terminal()` in
  `q2 add`/`create`/`use`/`provide-hub`) controls interactivity, not styling.
  It is out of scope.

## Summary

The report is accurate and reproduces exactly: 162 SGR sequences and 2 OSC 8
hyperlinks on stderr, with or without `NO_COLOR=1`. It is a duplicate of
bd-be8ve52f, which was found while wiring the claude-notes CI gate. The
reporter's one-line fix (pass `stderr_use_color()` in place of `true` at
`render.rs:1447`) would work. The audit found more: q2 has **at least four
independent styling policies**, none of which agree, and two places where
ANSI gets baked into stored strings. We recommend fixing the bug through a
single, resolved-once output-style policy rather than a point fix.

## Reproduction

The fixture is `repro/index.qmd` (it includes a missing file). The script
`repro/count-escapes.sh` counts escapes per output mode. Run it from a copy
outside any q2 project. Inside `claude-notes/`, the notes project claims
`index.qmd` and the render is refused.

Observed with `target/debug/q2` built from `e14065e79` (stderr redirected to a
file each time):

| mode                              | SGR | OSC 8 | correct? |
|-----------------------------------|----:|------:|----------|
| diagnostics                       | 162 |     2 | no: not a tty |
| diagnostics, `NO_COLOR=1`         | 162 |     2 | no |
| diagnostics, `--json-errors`      |   0 |     0 | yes |
| tracing (`-vv`)                   | 980 |     0 | no: not a tty |
| tracing (`-vv`), `NO_COLOR=1`     |   0 |     0 | yes |
| `render --help` piped (clap)      |   0 |     0 | yes |
| summary line ("1 error")          |   0 |     0 | yes |

## Localization: every place that decides styling today

1. **Render diagnostics** (the bug).
   `crates/quarto/src/commands/render.rs:1447` passes a hard-coded `true` to
   `format_render_diagnostics_text`. At `render.rs:1745` that value sets both
   `.color()` and `.hyperlinks()`.
2. **Summary counts clause.** `stderr_use_color()` at `render.rs:635` checks
   `NO_COLOR` and `stderr().is_terminal()`, so this one is correct. It treats
   `NO_COLOR=` (empty) as set, but the spec says an empty value counts as
   unset.
3. **Pass-1 failure strings.** `ProjectRenderSummary.pass1_failures[].error`
   is a `String` built with `e.to_string()`
   (`crates/quarto-core/src/project/orchestrator.rs:708`). For a
   `quarto_core::ParseError`, `Display` calls `to_text(...)` with the
   **default** `TextRenderOptions` (color and hyperlinks on;
   `crates/quarto-core/src/error.rs:32-44`). So ANSI is baked in at the point
   the error is created. `failure_error_text` (`render.rs:2133`) then strips
   it out again when color is off. That is the `warning: profile-pass skipped`
   line in the repro.
4. **`quarto-error-reporting` defaults.** `TextRenderOptions::default()`
   turns color and hyperlinks on (crate 0.4.1, `diagnostic.rs:87`). Every
   `to_text(None)` call therefore emits styling unless the caller opts out.
   The `pampa` binary does this at 7 sites (`crates/pampa/src/main.rs:221` …
   `:583`).
5. **tracing.** `crates/quarto/src/main.rs:1752` uses `fmt::layer()` with no
   `.with_ansi(..)`. tracing-subscriber 0.3.23 honours `NO_COLOR` by itself
   but never checks for a tty, which explains the 980 escapes in a log file.
   `quarto-hub/src/main.rs:374,389` does the same.
6. **pampa ANSI writer.** `crates/pampa/src/writers/ansi.rs:174-199` detects
   hyperlinks with `supports_hyperlinks::on(Stdout)`, but colors default on.
7. **qmd-syntax-helper** uses the `colored` crate, which has its own env
   handling.
8. **clap** (`--help`) uses `anstream`, which is correct (see below).

Two side observations:

- Several integration tests set `NO_COLOR=1` for subprocess runs (for
  example `crates/quarto/tests/integration/components_render.rs:69`). They
  get plain output only because the subprocess's stderr is not a tty, not
  because q2 honours `NO_COLOR`.
- q2 never sets color env vars for child processes. It already strips the
  ANSI that Quarto 1's Lua filters emit when it parses their stderr
  (`crates/quarto-core/src/pandoc_filters/diagnostics.rs:22`).

## Open questions resolved during triage

**Is `NO_COLOR` a de facto standard?** Yes. It is an informal convention, but
a widely adopted one. The no-color.org text reads: "Command-line software
which adds ANSI color to its output by default should check for a `NO_COLOR`
environment variable that, when present and not an empty string (regardless
of its value), prevents the addition of ANSI color." It also says:

- user config files and per-invocation CLI flags **should override** it;
- it is about color only, not bold, underline or italic;
- it says nothing about hyperlinks or non-tty output.

Quarto 1 honours it (`external-sources/quarto-cli/src/core/lib/external/colors.ts:18`,
via `Deno.noColor`). So does the Rust crate ecosystem: tracing-subscriber,
`anstyle-query` and clap all check it.

**What is the surrounding best practice?** It is a small, stable set of
conventions:

- **Auto-detect per stream.** Color stdout and stderr independently, each
  only when that stream is a terminal and `TERM` is not `dumb`.
- **`CLICOLOR_FORCE` / `FORCE_COLOR`.** These force color even when the
  stream is not a tty, which CI log viewers want. The CLICOLOR page is now
  marked deprecated in favour of no-color / force-color and says to treat
  `FORCE_COLOR` as an alias for `CLICOLOR_FORCE`. `CLICOLOR=0` disables
  color.
- **A `--color=auto|always|never` flag** (cargo, git, ripgrep, clap) that
  overrides everything.
- **Precedence.** `anstream` 1.0.0 (`src/auto.rs:198`, already in our lock
  file through clap) resolves `Auto` in this order: `NO_COLOR` → never;
  `CLICOLOR_FORCE` → always; `CLICOLOR=0` → never; tty and (`TERM` supports
  color, or `CLICOLOR`, or `CI`) → always; otherwise never. An explicit flag
  sits above all of these.
- **Hyperlinks have no `NO_COLOR` equivalent.** The common practice, from the
  `supports-hyperlinks` crate (already a pampa dependency), is to emit OSC 8
  only on a tty whose terminal is known to support it, with
  `FORCE_HYPERLINK=1/0` as the override. Forcing color in CI should **not**
  force hyperlinks, because GitHub Actions shows OSC 8 as noise. That is the
  exact complaint in bd-be8ve52f.

**Does this justify consolidation, not just the one-line fix?** Yes. The
point fix would make item 1 match item 2, but four policies would still
remain:

- the tracing layer still disagrees on tty handling;
- `ParseError` still bakes ANSI into strings;
- the default `TextRenderOptions` still leaks styling at every new call site;
- `pampa` still ignores all of it.

Every future sink would have to remember to thread the flag. That kind of
drift is how the current bug happened: bd-6d9ew2up added the `color` switch,
and the `execute` path never received it.

## Outcome / recommended next step

Duplicate of **bd-be8ve52f**. Recommended shape, for the maintainer to
confirm:

1. **One resolver, run once.** Put a small `OutputStyle { color: bool,
   hyperlinks: bool }` per stream in `quarto-util` (or similar), computed in
   `main` from:
   - a new global `--color=auto|always|never` flag;
   - `NO_COLOR` (non-empty);
   - `CLICOLOR_FORCE` / `FORCE_COLOR` (non-empty and not `0`);
   - `CLICOLOR=0`;
   - `is_terminal()` and `TERM != dumb`.

   Use the `anstyle-query` helpers or `anstream`\'s precedence rather than
   hand-writing the env parsing. Set `hyperlinks = color && is_terminal &&
   supports_hyperlinks`, with a `FORCE_HYPERLINK` override. Only an explicit
   `--color=always` should turn color on in a pipe, and never hyperlinks.
2. **Thread the policy to every sink.** That means the render diagnostics
   (replacing `true` and `stderr_use_color()`), the tracing layer
   (`.with_ansi(style.stderr.color)`), `quarto-hub`, and the `pampa` binary.
3. **Stop baking styling into stored strings.** `ParseError`\'s `Display`
   should render plain. Pass-1 failures should keep the diagnostics
   structured, so the sink applies styling and `strip_ansi_escapes` in
   `failure_error_text` becomes unnecessary.
4. **Optional, upstream.** Consider flipping `TextRenderOptions::default()`
   to plain in `quarto-error-reporting`, so a forgotten call site fails safe.
   This is a breaking default for an externalized crate, so it goes through
   that repo.
5. **Tests.** Write these first, using TDD:
   - a binary-driven test that `NO_COLOR=1` yields 0 SGR and 0 OSC 8 even
     with forced color;
   - a test that `--color=always` to a file yields SGR but no OSC 8;
   - unit tests of the resolver's precedence table, injecting env and tty
     rather than reading the process.
6. **Docs.** Document `--color` and the env vars on the CLI docs page.

**Priority.** bd-be8ve52f is P3. Since this now has an external reporter
whose log-capture workflow is affected, P2 seems right.

## Verification commands used

```bash
gh issue view 1135 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build            # pre-flight on main @ e14065e79
cp -r claude-notes/issue-reports/1135/repro "$TMP/r" && cd "$TMP/r" \
  && Q=<repo>/target/debug/q2 ./count-escapes.sh
braid search NO_COLOR; braid show bd-be8ve52f
grep -rnE 'NO_COLOR|is_terminal|TextRenderOptions|to_text\(|fmt::layer' crates
```

Sources: <https://no-color.org/>, <https://bixense.com/clicolors/>,
`anstream` 1.0.0 `src/auto.rs`, `anstyle-query` 1.1.5 `src/lib.rs`,
`tracing-subscriber` 0.3.23 `src/fmt/fmt_layer.rs:308`.

## Cross-references

- bd-be8ve52f: the same bug, found from the claude-notes CI gate
- bd-7v3trpxn: where bd-be8ve52f was discovered
- bd-6d9ew2up: added the `color` switch to `TextRenderOptions`
- bd-sl79jjiq: `format_render_diagnostics_text` shared with the `q2 preview --static` overlay
- `.claude/rules/cross-platform.md`: the resolver should handle Windows; `anstyle-query` enables VT mode there
