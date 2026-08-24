# `q2 docs llms`: embed the docs-site llms.txt artifacts in the binary

**Strand:** bd-hwop1zii
**Date:** 2026-08-24
**Status:** All four phases complete and committed (`1fbc2b93` on
`braid/bd-hwop1zii-docs-llms-embed`). Full workspace suite and
`cargo xtask verify --skip-hub-build` green; end-to-end transcript
recorded below. Awaiting user review — not pushed, strand still open.

## Overview

Embed the llms.txt artifact set that `q2 render docs/` already produces into
the `q2` binary at build time, and expose it through a new `q2 docs llms`
subcommand (with `q2 agents-info` as a visible top-level alias) so LLM agents
can consume q2's documentation offline, straight from the binary they are
already driving.

`docs` is a namespace, not a command: bare `q2 docs` prints subcommand help,
so humans are never handed the agent-facing markdown dump thinking it is "the
docs". The namespace deliberately reserves room for a future human-facing
embedded-docs mechanism (`q2 docs serve` / `q2 docs open` — q2 already embeds
a SPA and ships an HTTP server in `quarto-preview`, so serving an embedded
rendered `_site` for offline human reading is a natural later extension).

The generation side shipped in v0.26.0 (bd-llms-txt-unimplemented-oih6z6j7,
commits `2c144619` + `b7bfeef8`; closed 2026-08-24): rendering `docs/` writes

- `_site/llms.txt` — sidebar-organized markdown index (titles + descriptions),
- one `.md` companion per page (254 pages, ~1.1 MB at time of writing),
- `_site/llms-full.txt` — reading-order concatenation (~544 KB).

This feature is therefore **staging + embedding + CLI plumbing** — no new
document processing. ~1.7 MB of raw text is noise next to the ~40 MB embedded
preview-SPA WASM.

**Prior art.** `braid agents-info` (naming precedent); the llms.txt convention
(Answer.AI spec; Mintlify auto-hosts `/llms.txt` + `/llms-full.txt`); offline
toolchain docs (`rustup doc`, `go doc`). No known SSG embeds its own rendered
docs in its binary for agent consumption — this is a novel affordance.

**Related strands:** bd-3n4fpr3g (expose companion href via
shortcode/metadata), bd-3ar95048 (section-heading flattening), bd-to3vh0od
(code-annotation text in companions), bd-hzsi (LLM skill for listing
migration).

## Design

### CLI surface

New `Commands::Docs` namespace in `crates/quarto/src/main.rs` with an `Llms`
subcommand, implemented in `crates/quarto/src/commands/docs_llms.rs`. A
visible top-level `Commands::AgentsInfo` alias forwards to `docs llms` with
identical arguments; its help line reads "alias for `q2 docs llms`" (kept for
guessability — an agent that knows `braid agents-info` will try it — but
`docs llms` is canonical, and braid itself may adopt the `docs llms` shape
later).

| Invocation | Output |
|---|---|
| `q2 docs` | subcommand help (never document content) |
| `q2 docs llms` | embedded `llms.txt` verbatim, preceded by a short preamble telling the agent each href is retrievable via `q2 docs llms <href>` |
| `q2 docs llms --list` | one line per page: `href<TAB>title` (titles from the companion's leading `#` heading) |
| `q2 docs llms <href>` | that page's `.md` companion |
| `q2 docs llms --full` | embedded `llms-full.txt` verbatim |
| `q2 docs llms --embed-info` | provenance: docs snapshot git commit + dirty flag, page count, placeholder-or-real (mirrors `q2 mcp --launcher-info`) |
| `q2 agents-info ...` | alias: exactly `q2 docs llms ...` |

`--full`, `--list`, `--embed-info`, and `<href>` are mutually exclusive
(clap `conflicts_with` group). `--embed-info` stays under `llms` for now;
promote it to `q2 docs embed-info` only when a second embed consumer (e.g.
`docs serve`) appears.

Href normalization for `<href>`: exact match on the `.md` href as printed in
`llms.txt` wins; otherwise normalize `.qmd`/`.html` extensions and
extensionless paths to `.md`, and accept a leading `./` or `/`. Miss → error
listing near-matches (or pointing at `--list`), nonzero exit.

### Staging: `cargo xtask build-agents-docs`

New xtask (module `crates/xtask/src/build_agents_docs.rs`):

1. Record provenance from git **before** rendering (see Phase 2's note on
   why), then `cargo run --bin q2 -- render docs` — an in-place render, as
   shipped; the scratch-output-dir idea in the original draft was dropped
   because the ledger is keyed to the real output dir.
2. Copy the ledger-listed artifacts (preserving the directory tree) into
   **`agents-docs-dist/`** at the repo root (gitignored, sibling convention
   to `q2-preview-spa/dist/`), clearing any stale tree first.
3. Write `agents-docs-dist/embed-info.json`: `{ "commit": "<git rev-parse
   HEAD>", "dirty": <bool> }`. No timestamp — keeps builds reproducible;
   commit + dirty is what staleness diagnosis needs. The page count is
   derived at runtime by walking the embed, so it cannot disagree with what
   was actually embedded.

Note: `_site/*.md` includes only llms companions plus any user-authored
resource `.md` files; the Q-5-28 ledger (`llms_post_render.rs` manifest)
records which files llms-txt generated. Staging copies the *generated* set —
reading the ledger rather than globbing, so a stray resource `.md` cannot
leak into the embed. (Confirmed usable in Phase 2; no fallback needed.)

### Embedding: `build.rs` on the `quarto` bin crate

Follows `crates/quarto-trace-server/build.rs` (env-var indirection for
`include_dir!`):

- If `agents-docs-dist/llms.txt` exists → embed `agents-docs-dist/`.
- Otherwise → embed a generated placeholder dir (an `embed-info.json` with
  `"placeholder": true`) and emit a cargo warning naming
  `cargo xtask build-agents-docs`. Fresh clones stay buildable.
- `rerun-if-changed` per file (the preview build.rs `watch_recursive`
  pattern), so re-staging triggers re-embed.
- Plain `include_dir!`, **no tar.zst** — 1.7 MB of text doesn't justify the
  archive layer (bd-rem4bpee machinery exists if that ever changes).

On a placeholder build, `q2 docs llms` (all modes except `--embed-info`,
which reports the placeholder state) exits nonzero with instructions to run
the xtask and rebuild.

### The staleness trap (same as preview SPA / MCP bundle)

A plain `cargo build --bin q2` embeds whatever `agents-docs-dist/` was last
staged. Fresh docs require the chain:

```bash
cargo xtask build-agents-docs   # render docs/, stage artifacts
cargo build --bin q2            # re-embed via include_dir!
```

Mitigations, all in scope:

- CLAUDE.md section alongside "Verifying Rust changes in `q2 preview`".
- `cargo xtask build-all` gains the staging step. Unlike the MCP bundle and
  the SPAs it runs **last**, after the Rust build — staging renders the docs
  with `q2` itself — and ends with its own `cargo build --bin q2`.
- Release runbook + Release workflow: stage once in `web-payloads` (the
  docs embed is target-independent, and a cross-compiled leg could not run
  the `q2` that renders it), ship it in that job's artifact, and **assert
  the embed is real** in each leg's verify gate (`q2 docs llms
  --embed-info` must report `source: real` at exactly the tag's commit).
  A `(dirty)` marker warns rather than fails: the docs still came from
  that commit, so blocking a release over it would trade a real shipment
  for a cosmetic signal.
- `--embed-info` is the runtime diagnostic.

### Decisions (settled in plan review, 2026-08-24)

1. **Placeholder-tolerant dev builds; hard check only in the Release
   workflow.**
2. **Permissive href normalization** (exact `.md` match first).
3. **Canonical name `q2 docs llms`**, inside a `q2 docs` namespace reserved
   for embedded-docs mechanisms generally; **`q2 agents-info` kept as a
   visible top-level alias**. Bare `q2 docs` prints help so humans aren't
   misled into thinking the llms dump is the embedded human docs. (User
   confirmed; braid parity is non-binding — braid is internal and may itself
   move to this shape.)
4. **Topic branch in the current checkout** (no worktree — single agent in
   the repo, and it keeps cargo cleanup simpler; user decision 2026-08-24):
   `braid/bd-hwop1zii-docs-llms-embed` off `main`.

Deliberately out of scope — filed 2026-08-24 as `discovered-from`
bd-hwop1zii:

- **bd-x248xpyh** — human-facing offline docs: `q2 docs serve` / `q2 docs
  open` (the reason `docs` is a namespace rather than a command).
- **bd-b6cocsxw** — `--json` output for `--list` / `--embed-info`.
- **bd-dn81ol95** — expose the embedded docs as `q2 mcp` tools
  (search/fetch).
- Any change to llms.txt generation itself (not filed; belongs to the
  existing llms strands).

## Phases and work items

### Phase 1 — Tests + lookup module (TDD first)

Pure logic, no embed yet: `docs_llms` module in the `quarto` crate whose
functions take an `include_dir::Dir` (so tests inject a synthetic dir).

- [x] Unit tests: href normalization table (`.md` exact, `.qmd`, `.html`,
      extensionless, `./`- and `/`-prefixed, backslashes, trailing `/`,
      dir→`index.md`, misses with suggestions)
- [x] Unit tests: `--list` extraction (href + `#`-heading title; page with no
      heading falls back to the stem)
- [x] Unit tests: placeholder detection (missing `llms.txt` or
      `embed-info.json` `placeholder:true`); error text names the xtask and
      the rebuild step
- [x] Unit tests: `--embed-info` rendering (real/dirty/missing-sidecar/
      placeholder)
- [x] Run tests, verify the not-yet-implemented ones fail as expected
      (15/15 failed on `todo!()` stubs)
- [x] Implement the module until green (15/15 pass;
      `crates/quarto/src/commands/docs_llms.rs`, synthetic `Dir::new`
      fixtures — no filesystem fixtures needed)

Note: Phase 1 leaves dead-code warnings until the CLI consumes the module,
and `cargo xtask verify` is `-D warnings`; Phase 3's CLI wiring therefore
lands in the same commit as Phase 1 rather than adding a throwaway
`#[allow(dead_code)]`. Execution order is 1 → 3 → 2 (build.rs embeds the
placeholder fine before the xtask exists; the integration test accepts
either state).

### Phase 2 — `cargo xtask build-agents-docs`

- [x] Implement staging (`crates/xtask/src/build_agents_docs.rs`: render
      `docs/` in place via nested `cargo run --bin q2 -- render docs`, copy
      the ledger-listed set, write `embed-info.json`) + 3 unit tests for
      `stage_files` (unlisted `_site` files skipped, stale dist cleared,
      missing-listed-file and ledger-without-`llms.txt` errors)
- [x] Wire into `crates/xtask/src/main.rs` (`BuildAgentsDocs`)
- [x] Run it; inspect `agents-docs-dist/` — see the run record below
- [x] Add `agents-docs-dist/` to `.gitignore` (verified with
      `git check-ignore`)

**Staging decision, resolved:** the ledger *is* usable — the render writes
`docs/.quarto/llms-manifest.json` (`{version, generated: [...]}`, 256
output-dir-relative forward-slash paths including `llms.txt` and
`llms-full.txt`), so the xtask copies exactly the generated set and never
globs. It also refuses a ledger that doesn't list `llms.txt`. No fallback or
caveat needed.

**Render-in-place, not a scratch dir:** the plan sketched rendering to a
scratch output dir to avoid disturbing `docs/_site`, but the ledger lives at
`docs/.quarto/llms-manifest.json` and is keyed to the real output dir; a
scratch render would desynchronize the two. Rendering in place is also
exactly what CI does, so the staged bytes are the shipped bytes.

**First run (2026-08-24):** `Rendered 254 of 254 files` → `agents-docs-dist/
staged: 257 artifacts` (255 `.md` companions + `llms.txt` + `llms-full.txt`),
1.7 MB, `embed-info {"commit":"e3b3d7d4…","dirty":true}`.

### Phase 3 — build.rs + CLI wiring

- [x] `build.rs` on the `quarto` crate: real-or-placeholder embed dir,
      `QUARTO_DOCS_LLMS_EMBED_DIR` indirection, per-file rerun-if-changed,
      cargo warning on placeholder (observed firing on the pre-staging build)
- [x] Clap `docs` namespace + `llms` subcommand + top-level `agents-info`
      alias; `commands/docs_llms.rs` dispatch to the Phase 1 module
- [x] CLI parse unit tests: alias ≡ canonical across all five tails; bare
      `q2 docs` yields `DisplayHelpOnMissingArgumentOrSubcommand`; seven
      conflicting-mode combinations are `ArgumentConflict`
- [x] Integration test `crates/quarto/tests/integration/docs_llms_cli.rs`
      (registered, alphabetized) driving the real binary; every test passes
      in BOTH embed states — verified by running the suite before staging
      (placeholder, 8/8) and after (real, 8/8)
- [x] `cargo nextest run --workspace` — 13028 tests, all passing (1 leaky,
      pre-existing)

**Bug found and fixed during Phase 3 verification (TDD):** piping the output
(`q2 docs llms --full | head`) panicked with "failed printing to stdout:
Broken pipe" — Rust ignores SIGPIPE, so an unguarded `print!` aborts when the
reader goes away. For a command whose entire purpose is being piped into
agents, `head`, and `grep`, that is a real defect. Regression test
`closed_stdout_exits_quietly_without_panicking` written first (reproduced the
panic), then fixed by routing all output through a `write_stdout` helper that
treats `ErrorKind::BrokenPipe` as success.

**Binary-size measurement:** debug `q2` is 143 MB; the 1.7 MB embed is ~1.2%
of it — the "noise next to the WASM" premise holds.

### Phase 4 — Docs, wiring, verification

- [x] Docs page `docs/guides/embedded-docs.qmd` + `_quarto.yml` sidebar entry
      (placed in the flat Guides section right after `projects/llms-txt.qmd`,
      the feature it mirrors) — user-facing usage, not internals. Verified by
      rendering: 255 of 255 files, page appears in `llms.txt` with its
      description, and gets its own `.md` companion. Re-staged so the
      embedded corpus includes it (256 pages).
- [x] CLAUDE.md: staleness-trap section (after the `q2 mcp` one), noting the
      twist that this embed is staged *after* the Rust build
- [x] `cargo xtask build-all`: docs step added last (it needs the built
      `q2`), ending with its own `cargo build --bin q2`;
      `--skip-agents-docs` opt-out; implied off by `--skip-rust-build`.
      `cargo xtask verify` has its own config and is unaffected — the docs
      render does not slow the verify path.
- [x] Release runbook + Release workflow: staging step in `web-payloads`
      (target-independent, uploaded with the SPA dists and downloaded by
      every build leg), plus a verify-gate assertion that the binary reports
      `source: real` at exactly the tag's commit with no `(dirty)` marker
- [x] Pre-commit review checklist (`claude-notes/instructions/review.md`):
      no HashMap/serialization concerns, `cargo fmt --check` clean, clippy
      clean (two warnings of mine fixed: `write_with_newline`,
      `map_unwrap_or`), `cargo xtask lint` clean, no TODOs, no secrets
- [x] End-to-end verification (protocol below), transcript recorded here
- [x] `cargo xtask verify --skip-hub-build` — all 14 steps passed. Nothing
      under `quarto-core` moved, so the WASM leg is unaffected; the changed
      crates are `quarto` (bin) and `xtask` only.
- [x] File follow-up strands as `discovered-from` (bd-x248xpyh,
      bd-b6cocsxw, bd-dn81ol95)
- [x] Committed as `1fbc2b93` on `braid/bd-hwop1zii-docs-llms-embed`
      (19 files, no snapshot files added or modified)
- [ ] Close bd-hwop1zii (after user review / merge)

## End-to-end verification protocol (Phase 4 gate)

Per CLAUDE.md, tests are necessary but not sufficient. Before declaring done:

```bash
cargo xtask build-agents-docs
cargo build --bin q2
cargo run --bin q2 -- docs                        # subcommand help, no content
cargo run --bin q2 -- docs llms                   # llms.txt + preamble
cargo run --bin q2 -- docs llms --list            # 250+ href/title lines
cargo run --bin q2 -- docs llms guides/projects/llms-txt.md
cargo run --bin q2 -- docs llms guides/projects/llms-txt.qmd  # normalization
cargo run --bin q2 -- docs llms --full | wc -c    # ~550 KB
cargo run --bin q2 -- docs llms --embed-info      # real, correct commit
cargo run --bin q2 -- docs llms no/such/page      # nonzero + helpful error
cargo run --bin q2 -- agents-info --list          # alias ≡ docs llms --list
```

Inspect actual output (not absence of errors) and paste representative
snippets into this file.

### Observed transcript (2026-08-24, debug `q2`, real embed)

Output inspected, not merely checked for exit status.

```
$ q2 docs                                 # namespace help, never content
Documentation embedded in this binary
Usage: q2 docs [OPTIONS] <COMMAND>
Commands:
  llms  Machine-readable documentation (llms.txt) for AI agents
  help  Print this message or the help of the given subcommand(s)

$ q2 docs llms --embed-info
source: real
commit: e3b3d7d4aae422274a7f3ff1c781019c24bfbb07 (dirty)
pages: 256

$ q2 docs llms | head -12
<!--
q2 embedded documentation index (llms.txt).
Fetch one page:  q2 docs llms <href>   (hrefs listed below)
List all pages:  q2 docs llms --list
Whole corpus:    q2 docs llms --full
-->

# Quarto 2

> Documentation for Quarto 2, the Rust-based rewrite of the Quarto publishing system

## Guides

$ q2 docs llms --list | wc -l
     256
$ q2 docs llms --list | grep embedded
guides/embedded-docs.md	Embedded documentation for AI agents

$ q2 docs llms guides/embedded-docs.md | head -3
# Embedded documentation for AI agents

## Overview

$ q2 docs llms guides/embedded-docs.qmd | head -1   # .qmd normalization
# Embedded documentation for AI agents

$ q2 docs llms --full | wc -c
  560949

$ q2 agents-info --list | head -1                   # alias
about.md	About

$ q2 docs llms guides/embedded                      # miss
Error: no embedded documentation page matches `guides/embedded`; did you mean one of:
  guides/embedded-docs.md
Use `q2 docs llms --list` to see every page.
exit=1
```

The `(dirty)` marker is correct here — this transcript was taken from the
feature branch with uncommitted work. Release builds assert its absence.

## Progress log

- 2026-08-24: Plan written. bd-llms-txt-unimplemented-oih6z6j7 confirmed
  shipped (v0.26.0) and closed; its open child bd-to3vh0od re-linked as
  discovered-from. bd-hwop1zii filed and linked to related strands.
- 2026-08-24 (plan review): naming settled with user — canonical
  `q2 docs llms` inside a `q2 docs` namespace (room reserved for future
  human-facing `docs serve`/`open`), plus visible top-level `agents-info`
  alias. Plan updated throughout; strand title/description updated.
