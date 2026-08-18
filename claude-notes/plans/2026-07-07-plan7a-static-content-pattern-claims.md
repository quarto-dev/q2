# Plan 7a — Static content-pattern file claims (TOMBSTONE — superseded by Plan 7b)

**Status:** SUPERSEDED (2026-07-08, session "spin-parse-rust"). Do not execute this plan.
**Superseded by:** [2026-07-08-plan7b-native-content-processors.md](2026-07-08-plan7b-native-content-processors.md)
**Series root:** [2026-06-27-plan7-native-percent-spin-sourceinfo.md](2026-06-27-plan7-native-percent-spin-sourceinfo.md)

---

## Why this plan was withdrawn

7a proposed letting each engine declare an **arbitrary static regex** (`content-pattern`) on a
`claims-files` entry, evaluated natively at claim time and at Pass-1 discovery. The claim half was
sound (a regex over bytes is pure and load-free), but two problems surfaced:

1. **It only made the *claim* load-free, not the *conversion*.** 7a explicitly left conversion to
   Plan 7, which converted TS-engine percent scripts **over the wire** — so admitting a percent
   script into a project still launched Deno (or, for knitr spin, Rscript) in Pass-1's
   `markdown_for_file`. The perf goal ("no engine launch in Pass-1") was not actually met. See Plan 7b
   § Context for the code path (`engine_claims_file.rs:142` → `ts_process.rs:519`).
2. **Arbitrary per-engine regex was more surface than the problem needed.** The Q1 census that
   motivated 7a *also* proved every real content claim is exactly **percent or spin**. So a small
   registry of **named** processors (whose sniff regex lives in Rust) covers 100% of real cases with
   far less surface — no per-engine regex authoring, no regex-flavour choice, no ReDoS analysis.

Plan 7b replaces the arbitrary-regex `content-pattern` with a `processor:` **name** on the
`claims-files` entry (`percent` / `spin`), and makes both **sniff and convert** native and
engine-agnostic — so Pass-1 is genuinely launch-free.

## What migrated into Plan 7b (the parts we kept)

- **`processor:` schema field** on `claims-files` (replaces `content-pattern`).
- **Native Pass-1 discovery admission** via the processor's `sniff` (7a Stage 6).
- **One-predicate-two-sites coherence** — the same sniff at discovery and claim time (7a Stage 4).
- **Built-in claims as construction-free static data**, readable at discovery without launching
  engines (7a Stage 5).
- **The Q6 membership-cache contract** — content-dependent project membership can flip on a plain
  content edit; a future freeze/incremental cache must key on the content-derived admission bit, not
  filenames alone (7a Open Q6). Written into the DocumentProfile/freeze design notes; not implemented
  (no membership cache exists today).
- **The two-engine `.r`/`.R` tie-break** (7a Q3) — jupyter-percent vs knitr-spin, resolved by
  `contribution_order` first-match.

## What was withdrawn

- The `content-pattern` **arbitrary-regex** declaration and its whole-file/bounded-read, regex-flavour
  (Rust vs JS), multiline-flag, and ReDoS discussions — moot once the sniff moved into named Rust
  processors.
- The framing that a content claim is "the one genuine must-load case" — retracted in
  `engine-resolution.md §3.3` by Plan 7b (the genuinely-dynamic `claims_file` residue is the only
  must-load path, and it is *excluded* from Pass-1 discovery entirely).

> Historical detail (the full 7a design, schema ratification, and stage-by-stage write-up) is
> preserved in git history at this file's pre-2026-07-08 revision if ever needed.
