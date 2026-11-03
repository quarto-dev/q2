---
title: 'RIU-4: CLI surface for record sources'
date: 2026-11-02
description: 'Harden the --records option on q2 preview and q2 provide-hub (RIU-3a added a minimal version): validation, per-command defaults, the provide-hub rules and conflicts, the provider implementation of the resolver trait and its wiring into the request path, the staleness check of a q2 record when chosen, the diagnostics built from RIU-2 outcomes, and the documentation. The never-execute guard itself is RIU-2 (claim_and_spawn). The q2 record export-positron subcommand lives in RIU-5.'
status: ready  # nothing started; written from RIU-0 S1 B8 and S6, checked against the tree on 2026-11-03; the option name and default list are settled; revised after the fourth review (the guard moved to RIU-2, the flag split with RIU-3a)
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Evidence:** [RIU-0 findings](../research/2026-11-03-riu-0-spikes-findings.md), S1 B8, S6, S7;
[RIU-0c findings](../research/2026-11-03-riu-0c-findings.md), spike 4 (the clap option, the `Extension` and the guard were type-checked).
**Depends on:** RIU-2 (the source list type, the walk with its outcomes, the `RecordLookup` trait, the
execution guard and `ReExecuteConfig`; the eager-driver `off` guard move) and RIU-3a (a non-engine source and
the minimal flag this plan hardens).
**Blocks:** RIU-5 (its subcommand uses this plan's `--records` parsing), RIU-6. It does **not**
wait on RIU-5: the export subcommand lives there.
**Estimated sessions:** 1.5 to 2 (an estimate after the fifth review: the guard left, but the provider
implementation and its wiring into `execute_document`, the diagnostics and the documentation arrived).

> **For implementers:** tests first, through the real argument parser.

## Overview

Records can now come from several places. The user needs to say which, in what order, without a
new flag per source. Epic decision 4: sources are an ordered list, per document, first wins, no
merging. RIU-2 defines the list type, the walk and the execution guard, and RIU-3a added a minimal flag; this plan
validates the option, implements the resolver for the provider, turns the walk's outcomes into messages, and
documents it.

## The option (settled: name `--records`; defaults `cache,q2,engine` for preview, `engine` for provide-hub)

`--records <list>` on `q2 preview` and `q2 provide-hub`:

| Name | Source |
|---|---|
| `cache` | the preview cache (RIU-2) |
| `q2` | the copy under `.quarto/execution-records/` (RIU-2) |
| `positron` | the Positron inline-output cache (RIU-3) |
| a path (`./notes`, `/abs/dir`, `~/x`) | a directory of Positron-format `.ipynb` cache files, read by RIU-3's importer and locator, matched by file name, `quarto_source` or content hash |
| `engine` | run the engine |

For a document, the first source that supplies a record wins; `engine` last means "run only if
nothing else supplied one". A source **supplies** a record only if it yields one that describes
something and, for a source that matches by unit, at least one unit matches (RIU-2). Omitting `engine`
means never run the engine for that command.

**Parsing (revises epic decision 4: there is no `dir:` prefix).** The option is a comma-separated list
or a repeated flag, an ordinary `value_delimiter = ','` option like the others in `main.rs`
(`:825`, `:841`, `:847`, `:855`). Each entry is classified by its text alone; the parser does not touch
the filesystem:

1. A known name (`cache`, `q2`, `positron`, `engine`) is that source. Names win.
2. Otherwise an entry that looks like a path (contains `/` or `\`, starts with `.` or `~`, or is
   absolute, which includes a Windows drive path) is a directory source.
3. Otherwise it is an error that lists the valid names and says to write `./name` for a directory.

A directory source is checked once at start: a missing path, or a path that is a file, fails with a
clear message. A path that contains a comma cannot be given (nothing escapes it); the error says so. A
directory named like a source needs `./` in front. `cache` in a `provide-hub` list is a start-up error
(that command has no cache directory). A relative path resolves against the working directory of the
command.

The default is per command and defined once, in RIU-2's type: `q2 preview` is `cache,q2,engine`, with `q2`
in it on purpose (decided 2026-11-06), so a record file that passes the staleness check at choice can skip the
engine in a preview, as the epic's decision 3 states; `q2 provide-hub` is `engine`, because the provider's Decision #4 is a fresh, uncached run
(`execute.rs:332-335`) and it has no cache directory. A user opts into more with
`--records q2,engine` (with the gate off, which is the default until the `provide-hub` flip, the `q2` record is
refused at the publish seam and the run falls through to the engine, so set `QUARTO_ENGINE_RECORD_WRITE=1` for it to take effect).

## "Never execute" (settled by S1 B8)

`preview.engine: off` is the switch; no new flag. "Never execute" means no cell or engine code runs, as defined in epic decision 5: no code-cell or
inline-expression code and no engine process. The pre-engine pipeline (Lua filters, shortcodes, includes)
still runs, as in any preview and as in the provider's consent review. Project pre-render scripts still run
once at boot (`quarto-preview/src/lib.rs:445-447` runs them with no check of the policy or the list); a
guest cannot trigger them, and they are never re-run. The docs say so. Three things make it true:

1. **The source lookup runs outside the eager driver's `off` guard** (RIU-2 Phase E,
   `capture_driver.rs:67`). The watcher hook `recompute_staleness` (`:252`) has no lookup, only the
   staleness compare, and keeps its `off` guard (`config.rs:30-32` documents it as a no-op under `off`).
   With `off`, the cache, `q2`, `positron` and directory sources are consulted and the engine is never
   run. With `off`, `engine` in the `--records` list is ignored with a warning.
2. **Every re-execute path respects `off` and an absent `engine`, in one guard, and that guard is
   RIU-2's (Phase B3).** It sits in `claim_and_spawn`, which the HTTP handler and the automatic
   re-execute both use, so a `--share` guest is covered too. This plan tests it at the CLI level (the
   share test below) and documents it. The `EnginePolicy` is built once in
   `crates/quarto/src/commands/preview.rs:333-337` and already rides `PreviewConfig`.
3. **What a file change does.** The watcher (`lib.rs:499`) calls only `recompute_staleness`: it compares
   the existing record to the changed file and flags it. The source list is **not** run again on a file
   change, in the same way pre-render scripts and the eager driver run once at boot (`lib.rs:449` is the
   only production call; `re_execute.rs:391` is a test helper, `seed_capture`). So a Positron cache written
   after the preview started is not seen until the preview restarts, and a `.qmd` added later is not
   recorded. With a persisted `--data-dir` it is stickier: `record_eager_captures` skips any path that
   already has a `CaptureRef`, so after an engine re-execute the ref points at the engine record and the
   Positron one does not return, even across a restart. "Clear results" in the editor UI removes the record
   for every collaborator and nothing re-reads the list afterward. Document all three as known limits.

**The provider.** A `provide-hub` started with `--records positron` must not run the engine on an editor
request either. The provider's implementation of the trait (below) refuses `run_engine` when `engine` is not
in the list, so the consent-gated `run_and_store` is unreachable, and `execute_document` reaches it only
through the walk.

## Decisions that shape this plan

From the epic's "Decisions recorded 2026-11-05", with the answers to the fourth review
([review 4](../research/2026-11-05-riu-plans-review-4.md)).

- [ ] **Flag ownership.** RIU-3a added the minimal flag (parse the list, set `PreviewConfig.record_sources`,
      no validation). This plan owns everything else: validation, defaults, the `provide-hub` rules, the
      conflicts and the docs. Each plan's checklist names the flag code it owns.
- [ ] **The provider calls the walk, and implements `RecordLookup`** (RIU-2's trait). Today
      `execute_document` calls `run_and_store` directly, so without this item the trait implementation would
      be dead code and `--records positron` would still reach consent and the engine. The rule: when the
      resolved list is exactly `engine` (the default), `execute_document` calls `run_and_store` as it does
      today, with no walk and no `SourceInputs`, so the default path does not change. When the list names
      any other source, `execute_document` computes the `SourceInputs` for the request, calls
      `resolve_records`, and on `Chosen::Run` publishes the record through RIU-3's provider method (with the
      staleness of RIU-3 Phase B) or on `Chosen::EnginePublished` returns the `ExecOutcome` that
      `run_and_store` produced. The implementation: `lookup(Cache)` answers nothing (`cache` in a
      `provide-hub` list is already a start-up error); `lookup(Q2)` reads the `.quarto` copy from the
      materialized project (nearly always empty); `run_engine` goes through `run_and_store` with the consent
      gate, ignores its `inputs` argument, returns `EngineResult::Published`, and refuses when `engine` is not
      in the list. `Provider::new` and `new_with_overlay` (`execute.rs:101,113`; one production caller and
      four tests in `tests/integration/execute.rs`) gain the gate parameter, read once in `provide_hub::run`,
      which also owns the `cache` start-up error beside `validate_mode` (so it is not a parser test). A
      directory-source check at start is a named validator called from both commands. The provider's
      `quarto-hub-provider/Cargo.toml` gains `async-trait.workspace = true`.
- [ ] **What runs before consent (checked 2026-11-05, settled: nothing new).** The `SourceInputs` for a
      request come from `compute_source_inputs`, which runs the pre-engine pipeline (Lua filters, shortcodes,
      includes) on the project materialized from hub files. That pipeline **already runs before consent
      today**: `write_review_file` (`execute.rs:441-472`) calls `compute_input_qmd` at `:448`, which is
      `compute_source_inputs(..).input_qmd` (`preview_record.rs:260-268`), because the reviewer must be shown
      the resolved document. So a non-engine source adds no pre-consent work; it only skips the prompt, since
      no cell or engine code runs. What does change, and the docs say so: a `provide-hub` with a Positron
      source publishes the host's own Positron outputs to every viewer of the hub document without a
      prompt, so the host chooses that deliberately by naming the source.

## Phase A: tests first

- [ ] Parser tests (pure string tests, portable): `--records cache,q2`, repeated flag versus comma
      list, a bare path with a separator, a leading `./`, `~` and a Windows drive path
      (`C:\notes`) each become a directory source, a known name beats a same-named directory, a bare
      word that is not a name (`postiron`) is an error that lists valid names and suggests `./postiron`,
      a path with a comma is an error that says so, empty list is an error. Start-up tests (temp directory):
      a missing path and a file path are each refused with a clear message, and `cache` in a `provide-hub`
      list fails in `provide_hub::run` (not in the parser, which does not know the command).
- [ ] Resolution tests with fake sources are RIU-2's; here add that the parsed list reaches the walk and an
      absent `engine` means `run_engine` is never called.
- [ ] `preview.engine: off` plus `--records cache,positron,engine` does not run the engine and
      does show a cached record (depends on RIU-2 Phase E), and logs the warning about `engine`.
- [ ] The re-execute guard tests (the POST under `off`, the POST with `engine` absent, the automatic
      path, `manual` and `auto` with `engine` present, two handlers with different configs) are RIU-2's
      (Phase B3). Here, only the `--share` guest test above, through the real flag.
- [ ] A unit test of the "no source supplies" diagnostic text from a hand-built outcome list: the
      per-source lines, the refusal naming `QUARTO_ENGINE_RECORD_WRITE`, silence for a prose-only document,
      and silence when a source did supply.
- [ ] A test that a `q2` record from an older document is published stale, and one for the current
      document is published fresh.
- [ ] A test that pins how the format of a knitr record affects staleness. `record_is_stale` hashes a
      profiled unit under the format the record was **recorded** for (`recorded_format`,
      `record/staleness.rs:156-165`), not the format of the page being served, so a `q2` record written by a
      docx render is not reported stale by an HTML preview because of its format. Spike S2 of the
      [stack-rebase spikes](../research/2026-11-05-riu-stack-rebase-spikes-findings.md) observed the tag
      (`quarto-output-format` is `docx` on every knitr cell of such a record; a Jupyter record carries none).
      What the spike could not observe is whether docx-shaped structured knitr output presents correctly in
      an HTML preview, because nothing reads a `q2` record yet; this test and the first real `q2` source
      run answer it. If it presents wrongly, that is a question for Gordon, not a rule decided here.
- [ ] Provider tests: `execute_document` with the list `engine` calls `run_and_store` and no walk; with
      `positron` only it publishes the imported record and never reaches the consent gate (a fake engine that
      fails if run, a provider built with `AlwaysReject`); `run_engine` refuses with `engine` absent; with
      `engine` present it still goes through the gate and the walk reports `EnginePublished`; a refused
      record that is followed by a supplying source writes the fall-through line to stderr; the constructor
      takes the gate.
- [ ] A test that a file change runs `recompute_staleness` only and does not consult the source
      list (a source fake that fails if called after boot).
- [ ] A test that `q2 provide-hub` with no `--records` resolves to `engine` only, and `q2 preview`
      to `cache,q2,engine`.

## Phase B: wiring

- [ ] `--records` as a clap option (`value_delimiter = ','`, `Append`, a per-entry `value_parser`
      following the classification above; spike 4's parse test passed for a repeated flag, a Windows
      drive path and an unknown name) on the `Commands::Preview` variant
      (`crates/quarto/src/main.rs:214`) and `Commands::ProvideHub` (`:688`), through `PreviewArgs`
      (`commands/preview.rs:28`), `ProvideHubArgs` (`commands/provide_hub.rs:31`) and `PreviewConfig`
      (`lib.rs:231`).
- [ ] The parser produces RIU-2's list type; the default is defined in one place.
- [ ] The provider's `RecordLookup` implementation, the call from `execute_document`, the `Provider`
      constructor parameter, the `cache` start-up error in `provide_hub::run` and the directory validator (see
      the decisions).
- [ ] The `q2` source's staleness check, the diagnostic from outcomes, and the three conflicts.

## Phase C: the export subcommand is not here

The `q2 record export-positron` subcommand belongs to RIU-5, which owns the export function it wraps.
Nothing here depends on it.

## Phase D: documentation

Ownership, so no page has two final authors: RIU-1 rewrites the `record: true` paragraph of
`execution-records.qmd` (what the flip changes); RIU-3 writes the first text for the `positron` source and
the editor-UI and share notes; this plan writes the final text of the option, the per-command defaults and
the limits, and revises RIU-3's text for the hardened flag.

- [ ] `docs/guides/authoring/execution-records.qmd`: the `--records` option, usage and not internals. Say
      that `preview.engine: off` means no cell or engine code runs (no code cell, inline expression or engine
      process) while the document's filters, shortcodes and the project pre-render scripts still do, that sources are read at start and not again when a file changes, and the persisted `--data-dir`
      and "Clear results" limits above.
- [ ] `docs/guides/projects/preview.qmd`: the "Sharing" section (it says guests can run renders and
      downloads but not re-run code) must say that a guest can re-execute only if `engine` is in `--records`
      and the policy is not `off`; the "Code execution" and `preview.engine: off` text gets the same caveats.
- [ ] The share banner and help text (`share.rs:109-115` and `main.rs:274` say a guest can "RE-RUN" code)
      are false when `engine` is absent; make them say so.
- [ ] `q2 preview --help` and `q2 provide-hub --help` text, including the per-command defaults, that a
      relative directory path resolves against the working directory, and that `provide-hub` with a
      Positron source or a `q2` source needs `QUARTO_ENGINE_RECORD_WRITE=1` until the hosted hub has the
      reader.
- [ ] Refresh the embedded docs as a step, not a reminder: `cargo xtask build-agents-docs`, then
      `cargo build --bin q2`, then check `target/debug/q2 docs llms --embed-info` (the commit and `(dirty)`
      marker) and render `target/debug/q2 render docs/`. Do it before the release that carries the flip,
      since `q2 docs` serves the embed.

## Phase E: verify

Use `target/debug/q2`, never the `q2` on PATH (an old nightly that cannot read records).

- [ ] `cargo clippy -p quarto -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`
- [ ] `cargo nextest run -p quarto -p quarto-preview -p quarto-hub-provider`
- [ ] Real binary, **needs a browser for the page checks** and the RIU-3 fixtures, in the default viewer
      (the product's main surface): `q2 preview --records positron <doc>` on the RIU-3 Python fixture, with
      the locator pointed at the fixture directory (a bare-path source or the directory override; a
      hand-built cache is not at the URI-hashed name): no kernel starts, outputs show;
      `q2 preview --records cache <doc>` on a document with no cached record (nothing runs, and the page shows
      the "no source supplies" diagnostic with its per-source lines, which must be seen on the page); under
      `preview.engine: off`, `curl -X POST` to the re-execute endpoint is refused and no kernel starts (check
      the process list), as S1 B8 observed it starting one. The `curl` and process checks need no browser.
      Then the editor UI with `--share` and a guest: record what the guest can and cannot do (RIU-3 Phase G
      records the rest).
- [ ] The provider path is checked by its integration tests (a scripted `execute_document`); a real
      `provide-hub` against a local hub is run only if one is available, and the note says "not run" if not.
- [ ] The two-binary `--share` case (a guest on a different build than the host) was not run in RIU-0 (S3).
      Run it once before the flip ships, or say it was not run.
- [ ] Windows cross-check for the argument parsing and path tests:
      `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets --target x86_64-pc-windows-msvc -- -D warnings`.

## Exit criteria

- `--records` selects and orders sources on both commands, is validated, and the default is defined once.
- A `provide-hub` with `--records positron` cannot run the engine, because `execute_document` calls the
  walk and `run_engine` refuses; with the default list it behaves exactly as today. Its `cache` entry and a
  bad directory fail at start-up with a clear message. The docs state that a non-engine source publishes the host's Positron outputs without a consent prompt.
- The "no source supplies" diagnostic reports each source's outcome and is seen on the page.
- A `q2` record is staleness-checked when chosen.
- What a file change does, and the persisted-store and "Clear results" limits, are stated and tested where
  testable.
- The documentation pages above are written, the embed is refreshed, and the final text of the option is
  here, not in RIU-3.

## Risks

- **Option sprawl.** One option, one list. Anything else (a config key, per-document override)
  waits for a request.
- **A relative directory path** resolves against the working directory of the command, not the
  project. Say so in the help text.
- **A source that changes after boot is not re-read.** A stated limit, not a bug.
