---
title: 'RIU-4: CLI surface for record sources'
date: 2026-11-02
description: 'A --records option on q2 preview and q2 provide-hub that orders record sources per document (cache, q2, positron, dir, engine), a decision on whether preview.engine off already means never execute, and the smallest q2 record subcommands the Positron commands need.'
status: draft  # written 2026-11-02; the option name and default list are settled
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Depends on:** RIU-2 (the source list) and RIU-3 (a non-engine source to select).
**Blocks:** RIU-5 (exposes export), RIU-6.
**Estimated sessions:** 1.

> **For implementers:** tests first, through the real argument parser. The spelling below is
> a proposal; do not build until Gordon has decided it.

## Overview

Records can now come from several places. The user needs to say which, in what order, without
a new flag per source. Epic decision 4: sources are an ordered list, per document, first wins,
no merging.

## The option (settled: name `--records`, default `cache,q2,engine`)

`--records <list>`, comma separated, on `q2 preview` and `q2 provide-hub`:

| Name | Source |
|---|---|
| `cache` | the preview cache (RIU-2) |
| `q2` | the copy under `.quarto/execution-records/` |
| `positron` | Positron\'s inline-output cache (RIU-3) |
| `dir:<path>` | a directory of compatible `.ipynb` files, matched by `quarto_source` or file name |
| `engine` | run the engine |

Default: `cache,q2,engine`. For a document, the first source that supplies a record wins;
`engine` last means "run only if nothing else supplied one". Omitting `engine` means never run
the engine for that command.

Open points for review: whether `q2` belongs in the default list; whether `dir:` takes a
directory only or also a file; whether unknown names are an error (proposed: yes, with the list
of valid names); the option name itself.

## Phase A: tests first

- [ ] Parser tests: `--records cache,q2`, repeated flag versus comma list, `dir:` with a path
      containing a colon (Windows drive letters; split on the first `:` only), unknown name is
      an error that lists valid names, empty list is an error.
- [ ] Resolution tests with fake sources: first wins, a source that supplies nothing falls
      through, no merging, `engine` absent means the engine fake is never called.
- [ ] A test that `preview.engine: off` plus `--records cache,positron,engine` does not run the
      engine and does show a cached record. **Provisional (S1 B8, U5, and RIU-2 Phase E):**
      this depends on the record-source lookup sitting outside the `off` guard.

## Phase B: wiring

- [ ] `--records` as a clap option with `value_delimiter = ','` on the `Commands::Preview`
      variant (`crates/quarto/src/main.rs`, about `:214` on) and `Commands::ProvideHub` (about
      `:688`), through `PreviewArgs`, `ProvideHubArgs` and `PreviewConfig`. Confirm the line
      numbers; they are from a read, not a build.
- [ ] A `RecordSources` type in the shared crate from RIU-2, parsed from the option, passed to
      the seam. The default is defined in one place.
- [ ] Decide whether `preview.engine: off` suffices for "never execute". Proposal: yes. With
      `off`, `engine` in the list is ignored with a warning, and the other sources still run.
      If RIU-0 S1 shows `off` cannot be made to consult sources, add `--no-execute` instead and
      say why in this plan.

## Phase C: `q2 record` subcommands

Only what the Positron commands need; nothing speculative.

- [ ] `q2 record export-positron <doc.qmd> --out <file>` (wraps RIU-5\'s function). Exit code 0
      on success, a stable non-zero code per refusal class, a machine-readable one-line JSON
      result on stdout (`{"cells": n, "skipped": [...]}`) and diagnostics on stderr.
- [ ] If RIU-0 S6 shows the extension must read the q2 result differently, change the output
      shape here and nowhere else.

## Phase D: documentation

- [ ] `docs/` user page for `--records`, in the existing execution-records guide: usage, not
      internals.
- [ ] `q2 preview --help` and `q2 provide-hub --help` text.

## Phase E: verify

- [ ] `cargo clippy -p quarto -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`
- [ ] `cargo nextest run -p quarto -p quarto-preview -p quarto-hub-provider`
- [ ] Real binary: `q2 preview --records positron <doc>` on the RIU-3 fixture, and
      `q2 preview --records cache <doc>` on a document with no cached record (nothing runs,
      the page shows no outputs and says so).
- [ ] Windows cross-check for the argument parsing and path tests:
      `cargo clippy -p quarto --all-targets --target x86_64-pc-windows-msvc -- -D warnings`.

## Exit criteria

- `--records` selects and orders sources on both commands, and the default is defined once.
- "Never execute" is documented and tested.
- The export subcommand has a stable, documented contract.

## Risks

- **Option sprawl.** One option, one list. Anything else (a config key, per-document override)
  waits for a request.
- **`dir:` with relative paths** resolves against the working directory of the command, not the
  project. Say so in the help text.
