---
title: 'RFU-1: q2 convert --to positron-ipynb replaces q2 record export-positron'
date: 2026-11-08
description: 'Implements the stub q2 convert as the one conversion command, with --to required and -M accepted, and deletes the unreleased q2 record command. It has no option for the record source list (Gordon, 2026-11-09: that list becomes the metadata key execute: record-sources, read by OPR-5); until then it uses the fixed default q2,engine. Lands first, with no dependency on the one-presenter epic.'
status: in-progress
depends-on: []
lane: C
---

**Epic:** [RIU follow-up](2026-11-08-rfu-epic-riu-follow-up.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `rfu/1-convert-command` · **Lane C** (`workspace-5`) · **Blocks:** OPR-5, RFU-2, RFU-7, RFU-8, RFU-6.
**Needs:** nothing beyond the repository; Python for the Python fixture run (the tests skip cleanly without it).

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
git switch -c rfu/1-convert-command feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only rfu/1-convert-command
```

## Goal (Gordon, 2026-11-07)

Remove `q2 record`, which is too specific, and use `q2 convert`, as Quarto 1 had it, with the output format explicit.
Quarto 1 chose its direction from the input\'s extension
(`external-sources/quarto-cli/src/command/convert/cmd.ts`, in the main repo) and wrote beside the input. q2\'s `convert` is
a stub (`crates/quarto/src/commands/convert.rs` returns `NotImplemented`; its `--output` flag is documented as a
format, unlike Quarto 1\'s `-o` path). `q2 record` is unreleased (it landed in `10cfb0f93`), so removing it costs
nothing. The ipynb-to-qmd and plain qmd-to-ipynb sides are **not** part of this plan: strand `bd-ju7u8a73`.

**Design (agreed 2026-11-07):**

```
q2 convert <input> --to <format> [--from <format>] [-o <path>] [target options]
```

- `--to` is required and never inferred. `--from` is optional, inferred from the extension (`.qmd` and `.md` are qmd,
  `.ipynb` is ipynb) and an error otherwise. An unsupported pair is an error that lists the supported pairs. Names
  and flags match `q2 render` (`--to`, `-o`).
- Supported here: `--from qmd --to positron-ipynb`. Reserved by name, not implemented: `--to ipynb`, `--from ipynb`.
- Options for `positron-ipynb` only, rejected for other targets: `--source-uri`. There is **no option for the record source
  list** (decision R5, Gordon, 2026-11-09: it moves into metadata). `convert` uses the key `execute: record-sources`,
  allowed values `q2` and `engine`, default `q2,engine`, where `q2` alone forbids running the document; OPR-5 wires
  the key. Until OPR-5 lands, `convert` uses the fixed default `q2,engine`, defined in one place (a constant and one
  function) so OPR-5 replaces it with the reader.
- `-o <path>` is required for `positron-ipynb`, because the file name belongs to the consumer (a hash of the document
  URI, computed by the extension). `-o -` writes the notebook to stdout and the JSON report to stderr; otherwise the
  report is the one JSON line on stdout, as today.
- An existing output file is overwritten, with an info diagnostic naming it (Quarto 1 overwrote silently).
- Unchanged: `export_for_positron` in `quarto-core`, the report fields, exit codes 10 to 13 (documented under the
  target), exit 2 for usage errors (so the extension\'s "q2 too old" detection still works: an older `convert` has no
  `--to`). A `convert` that may run code says so in its help text.
- `-M KEY:VALUE` (repeatable) **is accepted**, parsed exactly as in `q2 render`, `preview` and `provide-hub` (YAML value,
  later repeat replaces, dotted keys not expanded) and applied as a runtime-metadata overlay, the way `provide-hub` applies
  it (`RenderOverlay::with_metadata(..).apply(runtime)`), to project discovery and to the engine run. It is how the
  record source list is set once OPR-5 lands (`-M 'execute:{record-sources: q2}'`). A `-M` overlay that reaches an
  engine run is recorded as given; do not strip it from the exported record.
- The `--source-uri` default is the document\'s path with forward slashes (`quarto_util::to_forward_slashes`). Today it
  is the native path (`commands/record.rs:~138-139`). `normalize_location` in `positron_cache.rs` accepts both forms
  and Positron ignores the field on load.

**Boundary with OPR-5.** This plan adds `-M` and **no** option for the source list: `convert` does not declare
`--execution-records`, and no command will (OPR-5 deletes `--records` from `preview` and `provide-hub`). Until OPR-5
lands, `convert` uses the fixed default `q2,engine`, so a user cannot yet choose `q2` alone; that interim state is
not released, so it is fine. OPR-5 then replaces the fixed default with the shared early reader.

## Checklist (tests first; every box is `[unattended]`)

- [ ] Read `crates/quarto/src/commands/record.rs` and `convert.rs`, `crates/quarto/tests/integration/record_export_positron_cli.rs`,
      and `crates/quarto-core/src/record/positron_export.rs`.
- [ ] **Tests first, through the binary** (`crates/quarto/tests/integration/`, replacing
      `record_export_positron_cli.rs`; register the new file in `main.rs`): `--to` missing is exit 2; an unsupported
      pair lists the supported ones; `--from` inferred from `.qmd`; an unknown extension errors;
      `-M` is accepted and reaches the engine run (a `-M` value changes the exported output); `--source-uri`
      is rejected for a target that does not take it; with no option the default `q2,engine` exports a document that has
      a fresh `.quarto` copy without running it; `-o -` puts the notebook on stdout and the report on
      stderr; an existing file is overwritten and an info line names it; exit codes 10 to 13 keep their meanings.
      **Move, do not weaken,** the existing cases. A test builds its expected `--source-uri` default with
      `Path::join` and forward slashes (cross-platform rule: a path that leaves the process uses `/`).
- [ ] Implement `commands/convert.rs` (replacing the stub) and wire `Commands::Convert` in `main.rs` with `--to`,
      `--from`, `-o/--output` (a path), `-M`, `--source-uri`. Delete `commands/record.rs`, the
      `Record` subcommand and `record_export_positron_cli.rs`.
- [ ] Run the real binary on a Python fixture and a Julia fixture
      (`target/debug/q2 convert d.qmd --to positron-ipynb -o out.ipynb --source-uri file:///x/d.qmd`); read the JSON and
      the ipynb; record the invocations and output in "Notes from the run". A knitr document still skips its cells
      (`EngineNotExportable`) until RFU-2: record that as the expected result, do not work around it.
- [ ] **Do not edit `docs/`** (RFU-8 writes the `q2 convert` page) and **do not edit the Positron fork** (RFU-7 updates
      its extension). Add a dated note, not a rewrite, to the command references in the RIU-5 and RIU-6 plans
      (`2026-11-02-riu-05-export-for-positron.md`, `2026-11-02-riu-06-positron-command-demo.md`).
- [ ] Gates: `cargo clippy -p quarto --all-targets -- -D warnings`; `cargo nextest run -p quarto`; the Windows
      cross-clippy for `quarto`; `cargo xtask lint`.

## Exit criteria

- `q2 convert d.qmd --to positron-ipynb -o out.ipynb` works on the Python and Julia fixtures and reports cells; every
  behavior in the design list has a passing test through the binary.
- `q2 record` no longer exists; `git grep -n 'export-positron'` outside `claude-notes/`, `docs/` and the fork shows nothing.
- `convert` takes its source list from one fixed default in one place that OPR-5 replaces with the key reader; it has no
  option for the list and accepts `-M`.

## Notes from the run

(Fill in at landing.)
