# Cargo dependency upgrade survey — 2026-05-04

**Worktree:** `.worktrees/cargo-upgrade-2026-05-04` (branch `cargo-upgrade-2026-05-04`, based on `main` \@ `3e0bc4c5`)
**Skill:** `.claude/skills/upgrade-cargo-deps/SKILL.md`
**Beads epic:** bd-hb8h
**Previous survey:** none (first run)

## TL;DR

- **Applied: 0** — `cargo update --workspace` reported `Locking 0 packages`. The lockfile was already at the latest in-range version for every dep. Nothing to commit on this branch.
- **Needs review: 16** major / pre-1.0-minor upgrades, each filed as a `chore` beads issue (see table below).
- **Skipped: 7** (vendored — consumed only by `crates/wasm-bindgen-futures-patch/`).
- **Surfaced but not filed: 28** patch/minor out-of-range deltas (workspace declares narrower ranges than the version constraint of upstream allows). These are non-breaking and listed below for reference; per the v1 skill, they don't get individual beads.
- **Duplicates baseline:** 49 distinct crates appear at multiple versions (108 crate-version entries). No after-state to compare since lockfile didn't change.
- **Verification:** pre-flight `cargo xtask verify --skip-hub-build` passed on `main` \@ `3e0bc4c5`. Worktree verify was skipped because the lockfile is identical to main's — see "Notes" below for the skill refinement this surfaced.

## Applied & verified

None this run. The lockfile was already current within all declared ranges.

## Needs review (major upgrades)

| Crate | Current | Available | Type | Beads |
|---|---|---|---|---|
| automerge | 0.8.0 | 0.9.0 | pre-1.0 minor | bd-tv2s |
| comrak | 0.50.0 | 0.52.0 | pre-1.0 minor (×2 steps) | bd-anhg |
| deno_core | 0.376.0 | 0.400.0 | pre-1.0 minor (large jump) | bd-nl5q |
| hmac | 0.12.1 | 0.13.0 | pre-1.0 minor | bd-fyuo |
| quick-xml | 0.37.5 | 0.39.2 | pre-1.0 minor (×2 steps) | bd-8356 |
| rand | 0.9.4 | 0.10.1 | pre-1.0 minor | bd-0a3b |
| reqwest | 0.12.28 | 0.13.3 | pre-1.0 minor | bd-v0zm |
| runtimelib | 1.6.0 | 2.0.0 | major | bd-tanz |
| scraper | 0.22.0 | 0.26.0 | pre-1.0 minor (×4 steps) | bd-9h2g |
| serde_v8 | 0.285.0 | 0.309.0 | pre-1.0 minor (large jump) | bd-rhs6 |
| sha1 | 0.10.6 | 0.11.0 | pre-1.0 minor | bd-gz6k |
| sha2 | 0.10.9 | 0.11.0 | pre-1.0 minor | bd-znva |
| similar | 2.7.0 | 3.1.0 | major | bd-zh24 |
| tree-sitter | 0.25.10 | 0.26.8 | pre-1.0 minor | bd-c083 |
| tree-sitter-highlight | 0.25.10 | 0.26.8 | pre-1.0 minor | bd-wjpd |
| ureq | 2.12.1 | 3.3.0 | major | bd-r9hs |

Each issue is `chore`, priority `3`, label `deps,cargo`, linked to bd-hb8h via `discovered-from`. Triage at your cadence.

## Skipped (vendored)

These are consumed only by `crates/wasm-bindgen-futures-patch/`, whose `Cargo.toml` is upstream's auto-generated file with `=` exact pins. Bumping any of these requires re-vendoring the patch, which is out of scope for this skill.

- `js-sys` 0.3.85 → 0.3.97
- `web-sys` 0.3.85 → 0.3.97
- `wasm-bindgen` 0.2.108 → 0.2.120
- `wasm-bindgen-futures` 0.4.58 → 0.4.70 (the vendored crate itself)
- `wasm-bindgen-macro` 0.2.108 → 0.2.120
- `wasm-bindgen-macro-support` 0.2.108 → 0.2.120
- `wasm-bindgen-shared` 0.2.108 → 0.2.120

## Surfaced but not filed (patch/minor out-of-range)

These deltas exist because either (a) our workspace declares a narrower range than upstream is at, or (b) a transitive constraint pins us back. They're non-breaking. Bumping individually would mostly mean widening a workspace range. Not worth a beads issue per dep.

Patch-only deltas:
- `cc` 1.2.60 → 1.2.61
- `crypto-common` 0.1.6 → 0.1.7
- `digest` 0.11.2 → 0.11.3
- `hybrid-array` 0.4.10 → 0.4.11
- `idna_adapter` 1.2.1 → 1.2.2
- `libc` 0.2.185 → 0.2.186
- `matchit` 0.8.4 → 0.8.6
- `openssl` 0.10.78 → 0.10.79
- `openssl-sys` 0.9.114 → 0.9.115
- `psm` 0.1.30 → 0.1.31
- `rustls` 0.23.38 → 0.23.40
- `rustls-pki-types` 1.14.0 → 1.14.1
- `siphasher` 1.0.2 → 1.0.3
- `tokio` 1.52.1 → 1.52.2

Minor-out-of-range deltas (1.x crates):
- `data-encoding` 2.10.0 → 2.11.0
- `jupyter-protocol` 1.4.0 → 1.5.0
- `kqueue-sys` 1.0.4 → 1.1.0
- `luajit-src` 210.6.6+707c12b → 210.7.1+18b087c

ICU stack (9 crates, all ICU 2.1.x → 2.2.x):
- `icu_calendar` 2.1.1 → 2.2.1
- `icu_calendar_data` 2.1.1 → 2.2.0
- `icu_collections` 2.1.1 → 2.2.0
- `icu_locale` 2.1.1 → 2.2.0
- `icu_locale_data` 2.1.2 → 2.2.0
- `icu_normalizer` 2.1.1 → 2.2.0
- `icu_normalizer_data` 2.1.1 → 2.2.0
- `icu_properties` 2.1.2 → 2.2.0
- `icu_properties_data` 2.1.2 → 2.2.0

Pre-release transition:
- `zeromq` 0.6.0-pre.1 → 0.6.0-pre.2 (pre-release semver — case-by-case)

## Duplicate-version delta

Before: 49 distinct crates duplicated (108 crate-version entries).
After: same — lockfile unchanged.

(Re-running `cargo tree --duplicates --workspace --depth 0` would produce identical output.)

## Notes — skill refinements surfaced by this run

The skill worked end-to-end, but two refinements should land in v2:

1. **Skip worktree verify when `cargo update` is a no-op.** The skill's step 7 says "skip to step 8" if `Locking 0 packages`, but step 8 (full `cargo xtask verify`) on an unchanged lockfile is redundant — pre-flight already validated `main`, and the worktree branches from `main`. We saved \~10 minutes by skipping it. Update the skill to say: if no lockfile diff, skip directly to step 11 (file beads).
2. **Distinguish "major candidates" from "out-of-range patch/minor" in step 11.** The skill literally says "for each major-upgrade candidate from step 3, file a `chore` issue", and step 3 lumps patch/minor-out-of-range together with majors as "needs review". Filing 28+ beads issues for patch deltas like `libc 0.2.185 → 0.2.186` would be noise. The implicit rule applied here: only **major** + **pre-1.0 minor** entries get individual beads; patch/minor-out-of-range get a section in the plan. Make this explicit in the skill.

A third minor refinement: the worktree's `npm install` (skill step 6) wasn't needed for this run since no hub-client work happens in the no-change case. Conditional: only run `npm install` if step 8's verify will actually run.

## Provenance

- Skill: `.claude/skills/upgrade-cargo-deps/SKILL.md` (v1, written 2026-05-04 same day as this run)
- Design plan: `claude-notes/plans/2026-05-04-cargo-dependency-upgrade-skill.md`
- Survey raw output: `/tmp/cargo-upgrade-survey.log` (gitignored; output above is the canonical record)
