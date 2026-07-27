# `q2 create`: interactive prompting (bd-hh1erpfx)

**Strand:** bd-hh1erpfx (discovered-from bd-oa5kd2yr)
**Branch:** `braid/bd-oa5kd2yr-q2-create-command` (same PR #409 line)
**Parent plan:** `claude-notes/plans/2026-07-23-q2-create-command.md`
**Created:** 2026-07-23

## Overview

Add Q1-style interactive prompts to `q2 create`: when invoked on a real
terminal with arguments missing, prompt for the artifact type, project
type, directory, and title instead of erroring. Piped/CI/`--json`
invocations keep today's exact non-interactive contract (all 27 existing
integration tests must keep passing untouched — they run with pipes and
therefore exercise the non-interactive gate).

Q1 reference: `$CLI/src/command/create/cmd.ts:62–110` — prompting is
enabled iff interactive terminal ∧ not CI ∧ `--no-prompt` absent ∧ not
`--json`; `nextPrompt` loops until the options bag is complete.

## Design decisions

1. **Prompt library: `inquire` 0.9.4.** Its default backend is
   `crossterm ^0.29.0`, which is *already* in our runtime tree at
   exactly 0.29.0 (via `pampa`), so the add is inquire + `dyn-clone` +
   `fuzzy-matcher` only. (`dialoguer` would promote `console` — today a
   dev-only dep via insta — to a runtime dep.) inquire renders prompt UI
   on stderr, matching our "stdout is for results" contract; verify this
   during implementation and keep it as a test invariant where
   observable.
2. **Gate** (`allow_prompt`): `stdin` is a TTY ∧ `stderr` is a TTY
   (via `std::io::IsTerminal`) ∧ `CI` env var unset (Q1's
   `runningInCI`) ∧ `--no-prompt` absent ∧ `--json` absent ∧ `--list`
   absent. New `--no-prompt` flag on `Commands::Create` (Q1 parity);
   with it, missing args error exactly as today.
3. **Prompter seam, not library calls inline.** A small trait so the
   prompt *flow* is unit-testable without a PTY:

   ```rust
   pub trait Prompter {
       fn select(&mut self, prompt: &str, items: &[PromptItem])
           -> Result<usize, CreateFailure>;
       fn input(&mut self, prompt: &str, default: Option<&str>)
           -> Result<String, CreateFailure>;
   }
   pub struct PromptItem { pub label: String, pub help: String }
   ```

   `InquirePrompter` is the real implementation; tests use a scripted
   fake (queued answers + recorded transcripts). Cancellation
   (Esc/Ctrl-C) surfaces as a `CreateFailure` ("Create cancelled") →
   non-zero exit, nothing written.
4. **Trait hook:** `ArtifactProvider` gains
   `resolve_interactive(&self, args, cwd, dry_run, prompter)` —
   same output type as `resolve_cli`. It consumes whatever positionals
   were provided and prompts only for the gaps (Q1's `nextPrompt`
   contract, expressed imperatively). `run_human` picks
   `resolve_interactive` when the gate allows, `resolve_cli` otherwise.
   The artifact-type select itself lives in `run_human` (over the
   provider registry) — prompted even when only one provider is
   registered, so behavior doesn't change when `extension` lands.
5. **Prompt sequence for `project`:**
   - *Project type*: select over **implemented** choices, labels
     `"Website — A Quarto website with navigation"` (unimplemented
     choices are not offered).
   - *Directory*: text input, validated non-empty (Q1 prompts bare;
     `.` allowed).
   - *Title*: text input with default = the same defaulting rule as the
     non-interactive path (directory name; choice id for `.`).
     Accepting a prompted default is explicit consent — **no
     defaulted-title warning** on this path (the warning stays on the
     non-interactive path, where the user never saw the default).
6. **No behavior change** for: `--json` (never prompts, even on a TTY —
   machine contract), `--list`, complete positional invocations, and
   every non-TTY invocation.

## Work items

### Phase 1: Tests first (TDD)

- [x] Unit tests (in `commands/create/`, `#[cfg(test)]`): a
      `ScriptedPrompter` (queued answers, recorded prompt transcript)
      driving the flows:
      - no args → type select + directory + title prompts → correct plan
        (title from prompt, no warning in `warnings`);
      - choice given (`["website"]`) → only directory + title prompted;
      - choice + dir given → only title prompted, default shown =
        directory name (and choice id when dir is `.`);
      - accepting the title default → plan title = default, no warning;
      - cancel at any prompt → `CreateFailure` mentioning "cancelled";
      - select offers only implemented choices, in registry order.
- [x] `run_human`-level test for the artifact-type select (single
      provider still prompts) — via the same scripted seam.
- [x] Integration tests (binary, `tests/integration/create.rs`):
      - `--no-prompt` with missing args → same error contract as today
        (piped stdio would never prompt anyway; the flag must parse and
        behave);
      - `CI=1` env with missing args → error, no hang;
      - existing 27 tests keep passing untouched (the non-interactive
        contract).
- [x] Run all new tests; record expected failures (trait/seam absent).
      **Recorded 2026-07-23:** `cargo nextest run -p quarto -E
      'test(interactive_tests)'` fails to compile with
      `E0432: unresolved import super::prompter` and
      `unresolved import super::select_artifact` — the expected
      failure mode for a seam that doesn't exist yet (same mode as the
      bd-kuxzj8su API migration).

### Phase 2: Implementation

- [x] Add `inquire` to `crates/quarto` (workspace dep entry: 0.9.4,
      default features — the fuzzy filter is small and useful).
- [x] `commands/create/prompter.rs`: `Prompter` trait + `PromptItem` +
      `InquirePrompter` (stderr rendering, cancel mapping).
- [x] `ArtifactProvider::resolve_interactive` + `ProjectProvider` impl
      (shared `resolve` core reused; prompts fill gaps only).
- [x] `run_human`: `allow_prompt` gate (`IsTerminal` + `CI` +
      `--no-prompt`); artifact-type select; route to
      `resolve_interactive`.
- [x] `--no-prompt` flag on `Commands::Create`; `--json`/`--list`
      unaffected.
- [x] All Phase-1 tests green; full `-p quarto` suite green.
      **2026-07-23:** 8 interactive unit tests + 29 create integration
      tests pass; `-p quarto` 227/227; clippy clean. A typed choice is
      validated *before* any prompting so `q2 create project blog` on a
      TTY errors immediately rather than prompting first.

### Phase 3: Verification

- [x] `cargo build --workspace` + `cargo nextest run --workspace` +
      `cargo xtask verify --skip-hub-build` (Rust-only change; no
      shared-crate or WASM surface touched — quarto-project-create is
      unchanged). **Workspace suite 10387/10387; `cargo xtask verify
      --skip-hub-build` passed (2026-07-23).**
- [x] **Real-PTY e2e** — done 2026-07-23 via `/usr/bin/expect`
      scripts (kept in the session scratchpad; coverage in CI comes
      from the scripted-prompter unit tests):
      - **Full flow**: `spawn sh -c "q2 create > stdout.txt"` in a
        PTY; prompts appeared in order `Artifact type` (Enter on
        `project — Project`) → `Project type` (arrow-down to
        `Website — A Quarto website with navigation`, Enter) →
        `Directory` (typed `ptysite`) → `Title (ptysite)` (Enter,
        accepting the default). Exit 0; scaffold on disk with
        `website:\n  title: "ptysite"`; **stdout purity verified** —
        `stdout.txt` contained only the human result block (created
        files + render hint); every ANSI prompt frame went to stderr
        (inquire renders on stderr as designed).
      - **`--json` on a TTY**: spawned `q2 create --json` in a PTY,
        sent the directive + Ctrl-D — no prompt appeared, result JSON
        emitted, project created. The machine contract holds even on a
        terminal.
      - **Cancellation**: Esc at the first prompt → "Create cancelled"
        diagnostic, exit 1, nothing written.
- [x] Verify `--json` on a real TTY still reads stdin without
      prompting. (Covered above.)

### Phase 4: Handoff

- [x] Update this plan; commit on the PR #409 branch; braid comment.
      Strand stays in_progress until CI/user review signs off.
