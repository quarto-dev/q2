---
title: 'JOL-3: several kernels per document (deferred)'
date: 2026-10-28
description: 'Deferred outline of a plan that would let one document run each owned language on its own kernel, in strict document order, kept with its open design questions as the starting point if it is reopened.'
status: draft  # deferred (decided with Gordon, 2026-10-30): SoS covers mixed-language documents; kept as a design outline, not a TDD checklist
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md), as JOL-3 (Phase 3 of
[the Jupyter owned-languages epic](2026-10-17-jupyter-owned-languages-epic.md)).
**Depends on:** [JOL-2b](2026-10-28-jol-2b-ownership-gate.md) (ownership gate and kernel
lookup by language) and [EXR-2](2026-10-28-exr-02-unit-scanner.md) (which defines the
`kernel_for(unit)` seam, the `KernelRouter`, that this plan makes vary per unit). Wants
the fake kernel built in [EXR-3](2026-10-28-exr-03-jupyter-iee.md) and extended in
[JOL-5](2026-10-28-jol-5-docs-e2e.md) for its exit test.
**Blocks:** nothing in the records epic. Records carry per-cell kernel metadata regardless
([EXR-4](2026-10-28-exr-04-record-format.md), [EXR-5](2026-10-28-exr-05-jupyter-records-shadow.md)),
so they do not wait for this plan.
**Estimated sessions:** unknown until the design questions are answered (guess 3-4).

> **Status of this plan.** Deferred (decided with Gordon, 2026-10-30). The file stays;
> nothing in it is scheduled. Everything below is provisional: Q1 runs one kernel per
> document, so this is new behavior beyond Q1, and it would become a task list only
> after the decisions below are made. Nothing else in either epic needs it.

## Why deferred, and when to reopen

A multiplexing kernel (SoS), selected with the `jupyter:` key that
[JOL-2a](2026-10-28-jol-2a-jupyter-key.md) makes work, gets mixed-language documents
running, so a q2-managed set of kernels is not needed now. Reopen this plan if:

- SoS proves insufficient (for example subkernel environments are not applied, variable
  exchange is too limited, or plain `{ggsql}`-style fences cannot be routed without q2
  sending SoS\'s `cell_kernel`), or
- another multiplexing need appears that a single kernel cannot serve.

What stays in place regardless: the `kernel_for` seam
([EXR-2](2026-10-28-exr-02-unit-scanner.md)) and its policy
([JOL-2b](2026-10-28-jol-2b-ownership-gate.md)), and the per-cell `quarto_kernel` record
metadata ([EXR-4](2026-10-28-exr-04-record-format.md)). Caveat: that SoS works through q2
is **not verified**; JOL-2a is not built, and the earlier success (JOL epic, 2026-10-17)
used a shim kernelspec named `python3`. JOL-2a and
[JOL-5](2026-10-28-jol-5-docs-e2e.md) carry the manual check against the real `sos`
kernelspec (which has `language: sos`, verified 2026-10-30).

## Goal

A document with cells in two languages runs each language on its own kernel, cells
strictly in document order across kernels (so a file written by one cell can be read by the
next), with nothing else shared between kernels.

## Why this is a stretch goal

- Until it lands, a mixed-language document keeps one kernel. The ggsql allowlist fix
  already on main made this visible: a `python` cell followed by a `ggsql` cell runs both
  on the Python kernel and the second fails (it used to be left unexecuted). Verified
  2026-10-30 (q2 0.36.0-nightly.20261027, ggsql kernel 0.5.2): the failure is a
  SyntaxError, with or without `engine: jupyter`.
- The main alternative needs no q2 multi-kernel work: a multiplexing kernel chosen with
  `jupyter:` (SoS, tested at kernel level 2026-10-17 in the JOL epic research notes),
  expected to work once [JOL-2a](2026-10-28-jol-2a-jupyter-key.md) lands, which is not
  yet verified through q2. This plan competes with it: one kernel that multiplexes
  versus q2 managing several.

## Background

Verified in the tree on 2026-10-28 (`origin/main` at `2ee0bd966`) unless marked.

- `execute_blocks_inner` runs all blocks on one session obtained from one `kernel_name`
  (`crates/quarto-core/src/engine/jupyter/text_execute.rs:262-274`). After EXR-2 Phase 5 the
  unit loop asks a `KernelRouter` for a kernel per unit, which is the seam this plan uses.
  *Verified (pre-EXR-2).*
- Sessions are already keyed by `SessionKey(kernel_name, working_dir)`
  (`SessionKey` is defined at `engine/jupyter/session.rs:325`; `daemon.rs:49` is a doc
  comment describing the key and `daemon.rs:90` builds one), so several concurrent
  sessions are structurally possible; `kernel_scope()` shuts kernels down when the outermost scope drops
  (`text_execute.rs:80-86` holds it for the run). *Verified.*
- `FigureWriter::begin_cell` is keyed to document position, not kernel
  (`text_execute.rs:655`), so figure naming should be unaffected. *Verified by reading;
  not tested with two kernels.*
- Record and replay and preview capture assume one session per run (JOL epic Phase 3
  note). *Not verified; to be checked in [EXR-5](2026-10-28-exr-05-jupyter-records-shadow.md).*
- `.ipynb` input carries one notebook-level kernelspec and fences every code cell with that
  one language (`crates/quarto-core/src/engine/content_processors/ipynb.rs:212,259`: the
  kernelspec language is read at `:212` and passed to `cell_wrap` at `:259`). *Verified
  2026-10-31.*
- Kernel startup cost for documents that never need a second kernel must stay zero.

## Decisions already made (JOL epic, provisional)

- Decision 4: the scalar `jupyter:` keeps its Q1 meaning; a map form
  `jupyter: {python: python3, ggsql: ggsql}` overrides per language.
- Decision 6: kernels are isolated sessions; cells run strictly in document order across
  kernels; nothing else is shared.

## Open questions (to resolve with Gordon before planning in detail)

1. **Build or defer.** Decided: defer (2026-10-30). Reopen only if SoS shows gaps that
   justify q2-managed kernels (subkernel env not applied, variable exchange limits,
   `cell_kernel` selection).
2. **How `jupyter:` expresses several kernels.** Exact grammar of the map form: are keys
   language names, matched case-insensitively and through the same (possibly empty) alias
   table as fences? Does the reserved key `kernel` (JOL-2a) collide with a language named
   `kernel`? How do the scalar and the map combine?
3. **Session lifecycle.** Lazy start per distinct owned language; shutdown with the render
   kernel scope; behavior when the second kernel fails to start midway (abort the render, or
   run the remaining cells of the first kernel). Zero cost for single-kernel documents.
4. **Ordering guarantee under preview.** Strictly serial execution is the decision; does
   preview re-execute (host-only) keep it, and can kernels be started in parallel while
   cells still run serially?
5. **Error attribution.** Messages and the `Q-` diagnostics must name the kernel as well as
   the cell; where does a kernel crash mid-document leave later cells?
6. **Records and capture.** The record carries a per-cell kernel (a `quarto_kernel`
   metadata key reserved in [EXR-4](2026-10-28-exr-04-record-format.md)); never a
   notebook-level single kernelspec (Plan 7c conflicts). How does the fingerprint carry the
   per-kernel set, and how does a captured record replay when a kernel is absent?
7. **Mixed-language `.ipynb` input.** Does this plan apply to notebooks at all, or only to
   `.qmd`? What should a notebook whose kernelspec has no `language` do?
8. **Environment and working directory.** Do all kernels share `cwd` and `project_env`
   (the cross-kernel file handoff implies yes)? Per-kernel timeouts and interrupts?
9. **Several kernelspecs per language** (system Python versus venv, several Julia versions):
   is Q1\'s first-match plus venv rule enough once a map form exists?
10. **Spans.** Inline expressions of a language route to that language\'s kernel
    ([EXR-3](2026-10-28-exr-03-jupyter-iee.md)); a span of an unowned or kernel-less
    language errors rather than running on the first cell\'s kernel.

## Sketch of the approach (not a commitment)

1. The router (EXR-2\'s seam, with JOL-2b\'s lookup behind it) returns a kernel name per
   unit; the execution loop keeps a map from `SessionKey` to session and starts sessions
   lazily.
2. Units run in document order, each routed by `kernel_for(unit)`.
3. Per-cell kernel metadata flows into the record.
4. The fake kernel (EXR-3, extended in JOL-5) is parameterized by name so a fixture
   document can use two.

## Exit test (when it becomes a plan)

A fixture document with two languages runs on two kernels, in order, with a cross-kernel
file handoff: the first cell writes a file, the second cell, on the other kernel, reads it
and its output proves the order. Also: a single-language document starts exactly one
kernel; a missing second kernel is reported as JOL-2b does (an error in a multi-engine
document, a warning with the cell left unexecuted in a single-engine one), and whether
this plan should tighten that for several-kernel documents is an open question; kernel
names appear in diagnostics.

## Risks

- Scope creep: this plan reopens preview capture, records and fingerprints. Keep it
  deferred unless the reopen criteria above are met.
- SoS may be good enough, making the plan unnecessary (the current expectation).
- Behavior beyond Q1 means no reference implementation to diff against.

## References

- [The JOL epic](2026-10-17-jupyter-owned-languages-epic.md), Phase 3, Risks and
  "Research notes: SoS".
- `crates/quarto-core/src/engine/jupyter/text_execute.rs`, `session.rs`, `daemon.rs`.
