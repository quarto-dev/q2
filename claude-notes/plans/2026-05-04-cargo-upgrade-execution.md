# Cargo upgrade execution plan — 2026-05-04 majors

**Survey:** `claude-notes/plans/2026-05-04-cargo-upgrade-survey.md`
**Tracking:** bd-hb8h (parent), 16 children
**Status:** 12 of 16 merged to main (1 deferred, 3 paused). Stopped here per user request to avoid disturbing parallel work in the project.

## Merged-to-main summary

All 9 upgrade branches merged to main via `--no-ff` merge commits in
the order they were developed. Lockfiles were regenerated as needed
during conflict resolution. The WASM `Cargo.lock` was restored to
origin/main's pre-merge state at the end (the merge re-resolution
drifted it onto a wasm-bindgen 0.2.120 that conflicts with the
locally-installed wasm-bindgen-cli 0.2.108 and the vendored
`wasm-bindgen-futures-patch`\'s `=0.2.108` pins).

Final verification: `cargo xtask verify` (full, with hub-client +
WASM + trace-viewer) passes on the merged main.

This plan tracks execution of the 16 major / pre-1.0-minor cargo upgrades surfaced by today's survey. Each upgrade is its own beads issue and gets its own worktree branch (`deps/<crate>-vX-Y`) so they can be reviewed and merged independently.

## Why isolate per upgrade

Some of these will be one-line bumps; some will require API migration. If a hard one fails, easy ones already merged shouldn't have to wait. Worktree-per-upgrade keeps blast radius small and lets a future session pick up any specific issue without disturbing the others.

## Order (easiest → hardest)

Ordered by expected migration cost and by ecosystem grouping (when crates must move together):

| # | Upgrade(s) | Beads | Notes |
|---|---|---|---|
| 1 | similar 2 → 3 | bd-zh24 | One callsite (quarto-citeproc). Likely trivial. |
| 2 | tree-sitter + tree-sitter-highlight 0.25 → 0.26 | bd-c083, bd-wjpd | Paired. Wide use. Grammar-crate compat to verify. |
| 3 | sha1 + sha2 + hmac (RustCrypto trio) | bd-gz6k, bd-znva, bd-fyuo | Co-released. Ensure digest-trait compat. |
| 4 | quick-xml 0.37 → 0.39 | bd-8356 | Workspace dep. Two minor steps. |
| 5 | rand 0.9 → 0.10 | bd-0a3b | Common dep, breaking-changes typical. |
| 6 | scraper 0.22 → 0.26 | bd-9h2g | HTML scraper. Four minor steps. |
| 7 | comrak 0.50 → 0.52 | bd-anhg | Markdown parser (comrak-to-pandoc). |
| 8 | automerge 0.8 → 0.9 | bd-tv2s | Hub-client (WASM). Needs full hub verify. |
| 9 | reqwest 0.12 → 0.13 | bd-v0zm | HTTP client. |
| 10 | ureq 2 → 3 | bd-r9hs | HTTP client. Largest semver jump. |
| 11 | runtimelib 1 → 2 | bd-tanz | Jupyter runtime. |
| 12 | deno_core + serde_v8 | bd-nl5q, bd-rhs6 | Paired. Largest internal jump (24 / 24 minor steps). Highest risk. |

## Per-upgrade workflow

For each entry:

1. `br update <id> --status in_progress`
2. Read upstream changelog/release notes (web fetch the crates.io release page).
3. Find callsites: `grep -rn "<crate>" --include="*.rs" --include=Cargo.toml`.
4. Create worktree: `git worktree add -b deps/<crate>-vX .worktrees/deps-<crate> main` (and beads redirect).
5. Bump the version in `Cargo.toml` (workspace level if declared there, else crate level).
6. `cargo build --workspace` — read errors, migrate APIs.
7. `cargo nextest run --workspace` — fix test breakage.
8. `cargo xtask verify` (skip-hub-build unless WASM-touching).
9. Commit with a descriptive message including key API migrations.
10. Update this plan: check off the entry, link the commit.
11. `br close <id> --reason "merged: <commit-hash>"` (deferred until user merges branch).

## Sound implementation criteria (per user request)

- **Don't suppress** breaking-change warnings with `#[allow(deprecated)]` or `#[allow(...)]` when the migration path is clear. Follow the documented migration.
- **Don't pin** to an older version with a `# pinned: <reason>` comment unless there's a real blocker (and if so, file a follow-up beads issue capturing the blocker).
- **Update tests** that change due to behavior shifts — but only if the change is intentional per the upstream changelog. Unintentional behavior shifts mean we shouldn't merge.
- **Keep snapshot updates documented** per CLAUDE.md — count, summary, surprising changes.
- **End-to-end verify** any user-visible output change before declaring complete (per CLAUDE.md "End-to-end verification before declaring success").

## Progress tracker

- [x] 1. similar 2 → 3 (bd-zh24) — branch `deps/similar-3` @ `82c725f1`
- [x] 2. tree-sitter pair (bd-c083, bd-wjpd) — branch `deps/tree-sitter-026` @ `3c73b49d` (named_child API: usize → u32)
- [x] 3. RustCrypto trio (bd-gz6k, bd-znva, bd-fyuo) — branch `deps/rustcrypto` @ `9ee17613` (digest 0.10→0.11: GenericArray→Array, KeyInit trait split)
- [x] 4. quick-xml (bd-8356) — branch `deps/quick-xml-039` @ `3f6c765c` (BytesText::unescape removed; Event::GeneralRef coalescing)
- [ ] 5. rand (bd-0a3b) — **deferred, blocked by bd-tv2s (automerge)**. `ThreadRng` in rand 0.10 doesn't satisfy the rand 0.9 `Rng` trait that `automerge::DocumentId::new` consumes; the upgrade only makes sense once automerge/samod move to rand 0.10.
- [x] 6. scraper (bd-9h2g) — branch `deps/scraper-026` @ `d0d044ec` (one-line bump, no API change)
- [x] 7. comrak (bd-anhg) — branch `deps/comrak-052` @ `c3a96f21` (no API change)
- [ ] 8. automerge (bd-tv2s) — **paused** (parallel work in flight; revisit in a later session)
- [x] 9. reqwest (bd-v0zm) — branch `deps/reqwest-013` @ `ebd84b30` (rustls-tls feature renamed to rustls)
- [x] 10. ureq (bd-r9hs) — branch `deps/ureq-3` @ `24007314` (Error::Status removed, status→StatusCode, into_body() body API)
- [x] 11. runtimelib (bd-tanz) — branch `deps/runtimelib-2` @ `415999ab` (paired with jupyter-protocol 2; MediaType non_exhaustive)
- [ ] 12. Deno pair (bd-nl5q, bd-rhs6) — **paused** (largest semver jump; revisit when there's a dedicated session window)

## Session boundary protocol

If a session ends mid-upgrade:

1. Leave the worktree intact.
2. Update the in-progress entry above with what's done and what's left.
3. Don't `br close` the issue — leave it `in_progress`.
4. Next session resumes from this plan + the worktree branch state.

## Notes / discoveries

- **tree-sitter 0.26**: `Node::named_child(i)` changed from `usize` to `u32`. `named_child_count()` still returns `usize`, so loops need a `u32::try_from(count).unwrap()` cast on the bound. Pattern used: store the count as `u32` once at loop entry. Grammar crates (tree-sitter-python 0.25, tree-sitter-r 1.2, tree-sitter-bash 0.25, tree-sitter-css 0.25, tree-sitter-html 0.23, tree-sitter-javascript 0.25, tree-sitter-typescript 0.23, tree-sitter-json 0.24, tree-sitter-yaml 0.7) are ABI-compatible with 0.26 — no grammar updates needed.

- **runtimelib 2 forces jupyter-protocol 2**: runtimelib 2 re-exports types from jupyter-protocol 2 (`ConnectionInfo`, `JupyterMessage`, `JupyterMessageContent`, `MediaType`). If our direct dep stays at jupyter-protocol 1.4, the lockfile carries both versions and same-named types from the two crates are unrelated — every `runtimelib::*` callsite that takes a `jupyter_protocol::*` type fails to typecheck. Bump both deps together. Also: `MediaType` is now `#[non_exhaustive]`; matches need a fallback arm (we used `format!("{:?}", other)` for the mime to preserve unknowns rather than panic).

- **ureq 3**: Three breaking changes for callers. (a) Feature `tls` renamed to `rustls`. (b) `Error::Status(status, resp)` arm removed — non-2xx responses now arrive in the `Ok` arm; check `response.status()` yourself. (c) Response API: `status()` returns `http::StatusCode` (convert with `.into()` to `u16`), and `into_reader()` is gone — use `.into_body().into_reader()` to get an `impl Read`. The simplification of merging 2xx and non-2xx into a single arm is generally a behavior improvement.

- **reqwest 0.13**: feature flag `rustls-tls` was renamed to `rustls`. No source-code API changes. If a Cargo.toml carries `features = ["rustls-tls"]`, change it to `features = ["rustls"]`. Other features used here (`blocking`, `json`) are unchanged.

- **rand 0.10 blocked on automerge**: rand 0.10 changed `ThreadRng` such that it no longer implements the rand 0.9 `Rng` trait. Our direct `rand::rng().fill_bytes(...)` migrations are easy (`use rand::Rng;` instead of `use rand::RngCore;`), but `automerge::DocumentId::new(&mut rng)` expects rand 0.9's trait. Until automerge/samod ship with rand 0.10 support, we have to stay at rand 0.9 to keep the trait-generic API satisfied. Captured as `bd-tv2s blocks bd-0a3b`.

- **quick-xml 0.39**: Two breaking changes. (a) `BytesText::unescape()` removed — replace with `BytesText::decode()` (byte→str) followed by `quick_xml::escape::unescape` (entity resolution). (b) New `Event::GeneralRef(BytesRef)` event emitted separately for entity references; entities are no longer inlined into `Event::Text`. To preserve "one Text child per text run" semantics, parsers must coalesce consecutive Text + GeneralRef events into a buffer that flushes on structural events. Use `escape::unescape(&format!("&{};", entity_body))` to handle both named and numeric refs through one path. Watch out for `&#160;` (non-breaking space) — common in i18n/locale XML; `resolve_predefined_entity` won't handle it, only `unescape` does.

- **digest 0.11 (sha1/sha2/hmac)**: `finalize()` returns `hybrid_array::Array<u8, _>` instead of `generic_array::GenericArray<u8, _>`. The new type **does not implement `LowerHex`**, so `format!("{:x}", hash)` no longer compiles. Migration: use `hex::encode(hash)` (or an inline `for b in &bytes[..n] { write!(s, "{:02x}", b) }` helper in build scripts to avoid a build-dep). Hex output is byte-identical to the old `LowerHex` output. Also: `Hmac::new_from_slice` moved from the `Mac` trait to the `KeyInit` trait — add `use hmac::KeyInit;`. Worth checking if any crate has captured SHA-256 baselines in tests; the `quarto-core` artifact-scoping pipeline test passing on the migrated code is the byte-identical proof.
