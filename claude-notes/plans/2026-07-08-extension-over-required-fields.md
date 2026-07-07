# Relax q2 over-required `_extension.yml` fields (bd-8b0af414)

## Overview

q2's extension reader (`crates/quarto-core/src/extension/read.rs`) hard-requires
`title`, `author`, and the `contributes` key. Quarto 1 requires **none** of these
by name: `schema/extension.yml` has no top-level `required:` marker, `readExtension`
reads `title`/`author` leniently, and the only enforced rules are (a) `contributes`
yields ≥1 non-empty contribution and (b) the `quarto-required` semver check. A real
shipped Q1 extension (**julia-engine**) has no `author` and loads fine.

Consequence: an otherwise-valid Q1-era extension whose `_extension.yml` omits
`author` (or `title`) fails `read_extension` → `Err`; `discover.rs` logs only a
`WARN` and skips it (returns `Vec`, not `Result`), so the engine/filter never
registers and the document renders as raw code — misattributed to the user's doc.

Introduced in `68420002b` (2026-03-16) by transcribing TS Quarto's *documented*
field list into *required* status; TS Quarto never enforced it.

## Audit outcome (background agent, 10 shipped Q1 manifests + schema/reader citations)

| field | q2 hard-required? | Q1 required? | action |
|---|---|---|---|
| `title` | YES `read.rs:82-91` | NO (`title \|\| id.name` fallback, `extension.ts:738`) | **RELAX** → default to ext id name |
| `author` | YES `read.rs:93-102` | NO (julia-engine ships without it) | **RELAX** → `Option<String>` |
| `contributes` key present | YES `read.rs:114` | effectively (via count check) | **KEEP** (relaxing only changes an error message, not which extensions load) |
| `contributes` ≥1 contribution | YES `read.rs:189-198` | YES (`extension.ts:735-741`) | **KEEP** — Q1-faithful; on this branch engines are already parsed + counted |
| engine external `path` | YES `read.rs:384` | YES (`definitions.yml:303` `required:[path]`) | **KEEP** — Q1-faithful, deliberate |

> **Branch reconciliation:** the audit read the *main* checkout, where
> `parse_contributes` does not parse `engines`. This worktree is on
> `feature/ts-engine-extensions`, which **does** parse engines and counts them in
> the non-empty check, so the audit's "engines aren't counted" divergence does not
> apply here. revealjs-plugins parsing/counting remains absent on this branch —
> **accepted-untested / out of scope** (separate concern, not a transcription
> accident of this bug).

## Design decisions

- **`title` stays `String`**, defaulted to `ext_name` (the derived extension name)
  when absent — mirrors Q1's `extension.title || extension.id.name`. Chosen over
  `unwrap_or_default()` (empty string) so a consumer always has a sensible display
  name. Nothing in production currently reads `.title` (only tests), but the
  always-resolved model avoids future `unwrap` sites.
- **`author` becomes `Option<String>`** — faithful to Q1's `string | undefined`.
  Nothing in production reads `.author` (grep-confirmed: only test asserts).
  `Option` over default-`""` so "no author" is distinguishable from "empty author".

## Production hunks (named, for revert-binding)

- **H-title** — `read.rs:82-91`: `.ok_or_else(|| "missing required 'title'")?.to_string()`
  → `.map(|s| s.to_string()).unwrap_or_else(|| ext_name.clone())`.
- **H-author** — `read.rs:93-102` + `types.rs:59`: `.ok_or_else(|| "missing required 'author'")?.to_string()`
  → `.map(|s| s.to_string())` producing `Option<String>`; struct field `String` → `Option<String>`.
- **H-contributes-key** — `read.rs:114` `.ok_or_else(...)?` : **unchanged** (guard).
- **H-contributes-count** — `read.rs:189-198` : **unchanged** (guard).
- **H-skip** — `discover.rs:114`/`:136` `Err(e) => warn!(...)` skip branch : **unchanged** (guard).

## Frozen Test Seam Spec

All rows: **tier =** native in-process, real `read_extension`/`discover_extensions`
driven through the real YAML parser + `NativeRuntime` + a `tempfile::TempDir`
(the exact path `discover.rs`/`scan_extension_entry` uses at runtime). **Mock
boundary =** none; the unit under test is never mocked. Filesystem is real (tempdir).

| # | test (file) | real unit | seam (mount + assertion surface) | named revert → RED |
|---|---|---|---|---|
| **T1** | `test_read_extension_author_optional` (NEW, read.rs) | `read_extension` | write `_extension.yml` with `title` + non-empty `contributes.shortcodes`, **no `author`**; assert `.unwrap()` loads, `ext.author.is_none()`, `ext.contributes.shortcodes.len()==1` (contribution actually carried) | **Revert H-author** → missing-author errors → `.unwrap()` panics → RED. `title` present ⇒ H-title revert does *not* affect this test (clean isolation). |
| **T2** | `test_read_extension_missing_title` (FLIP existing, read.rs:507) | `read_extension` | write `_extension.yml` with `author` + non-empty `contributes`, **no `title`**; assert `.unwrap()` loads and `ext.title == "<dirname>"` (the ext id name) | **Revert H-title** → missing-title errors → panic → RED. `author` present ⇒ H-author revert does *not* affect. **Discriminator check:** `== ext_name` also reddens under a lazy `unwrap_or_default()` ("" ≠ dirname), pinning the id-name default specifically. |
| **T3** | `test_read_extension_q1_engine_shape` (NEW, read.rs) | `read_extension` | write julia-engine-shaped manifest: `title` + `version` + `quarto-required` + `contributes.engines: [{path: foo.js}]`, **no `author`**; assert loads, `ext.author.is_none()`, `ext.contributes.engines.len()==1` | **Revert H-author** → RED. Reproduces the exact confirmed failure (engines-only + no author) now loading. |
| **T4a** | `test_read_extension_missing_contributes` (KEEP, read.rs:530) | `read_extension` | manifest with `title`+`author`, no `contributes`; assert `Err` mentions `contributes` | Guard for **H-contributes-key** — revert it (make key optional w/o count fold) and this changes. Stays green under our change. |
| **T4b** | `test_read_extension_empty_contributes` (KEEP, read.rs:551) | `read_extension` | `contributes.formats:` empty; assert `Err` mentions "at least one" | Guard for **H-contributes-count**. Stays green. |
| **T5** | `test_discover_invalid_extension_skipped` (UPDATE fixture, discover.rs:336) | `discover_extensions` | change the "bad" fixture from **missing-title** (which now LOADS) to **absent-`contributes`** (still a read error); assert `len()==1`, name `good-ext` | **Revert H-skip** (`Err=>warn` → push placeholder) → finds 2 → RED, *provided* bad fixture still errors. Refactor-induced fixture change: without it the test goes vacuous (missing-title now loads → finds 2 → false failure). |

### Compile-forced edits (author `String` → `Option<String>`; mechanical, not behavior)

- `types.rs:59` `pub author: String` → `Option<String>`.
- `read.rs:123` constructor `author,` — variable now `Option<String>`, flows through.
- `read.rs:658` `assert_eq!(ext.author, "Test Author")` → `assert_eq!(ext.author.as_deref(), Some("Test Author"))`.
- `read.rs:397` (`test_read_minimal_extension`, the `ext.author == "Test Author"` assert) — same as above; verify exact line.
- Extension struct literals set `author: "...".to_string()`:
  `filter_resolve.rs:484`, `discover.rs:378/395/414/423/441/450`,
  `project/mod.rs:1601`, `stage/stages/metadata_merge.rs:1662` → wrap `Some(...)`.
- `transforms/shortcode_resolve.rs:2046` `author: String::new()` → `author: None`.
- (YAML-text `author:` inside raw-string fixtures and `document_profile`/`pipeline`
  front-matter are **not** Extension literals — no change.)

## Missing-test pass

- **revealjs-plugins** contribution parsing/counting: absent on this branch —
  **accepted-untested, out of scope** (separate from the transcription-accident bug).
- **`contributes`-key-present vs count-check message parity**: deliberately kept as-is;
  relaxing changes only an error message, not which extensions load — **accepted-untested**.
- **filter map-entry `path` silently dropped** (`read.rs` `filter_map`): pre-existing
  laxer-than-Q1 behavior, unrelated to this bug — **accepted-untested**.

## Checklist

- [x] T2: flip `test_read_extension_missing_title` → `_defaults_to_id_name` (loads, `title==ext_name`) — RED first ✓
- [x] T1: add `test_read_extension_author_optional` — RED first ✓
- [x] T3: add `test_read_extension_q1_engine_shape` — RED first ✓
- [x] T5: update `test_discover_invalid_extension_skipped` bad fixture to absent-contributes (stays green)
- [x] Verify T1/T2/T3 RED for the named-hunk reason (`missing required 'author'/'title' field`)
- [x] Implement H-title + H-author (read.rs + types.rs) + compile-forced edits → GREEN
- [x] fail-on-revert spot check: revert H-author → T1+T3 RED, T2 GREEN; revert H-title → T2 RED, T1+T3 GREEN
- [x] `cargo nextest run -p quarto-core` — 2703 passed (1 flaky julia e2e passed in isolation; baseline flake, not a regression)
- [x] `cargo nextest run --workspace` — 10667 passed, 0 failed
- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests` — **all Rust legs green**: Step 1 custom lints + clippy (`-D warnings`) pass (fixed `map_unwrap_or` → `map_or_else`); `cargo build --workspace` + full `cargo nextest run --workspace` = 10667 passed. Step 6 (ts-packages `tsc`) fails **only** on `TS2307 Cannot find module '@quarto/...'` = missing `node_modules` in this fresh worktree (no `npm install`, correctly skipped — hub-client not in scope). Diff is Rust-only (7 quarto-core files, zero TS/WASM/ts-packages); the extension reader is not in the WASM/TS path, so this leg is unaffected by the change. Run `npm install` at the worktree root if a full ts-packages pass is wanted.
- [x] End-to-end: `q2 render` author-less shortcode extension. BEFORE (author required): 4× `WARN Failed to read extension … missing required 'author'`, `{{< greet >}}` → `<strong>?greet</strong>` (unknown-shortcode fallback, silent exit 0). AFTER (fix): extension loads, no warnings, `{{< greet >}}` → `HELLO_FROM_AUTHORLESS_EXTENSION`. Fixture: `scratchpad/e2e-authorless/`.
