# Hub: contain index-sourced paths to the project root (path-traversal write fix)

**Issue:** bd-rz6yb (bug, p1)

## Overview

A connecting client (autosync, a browser, or any hand-rolled client) that
passes authentication can write the project's **index document** directly
over the Automerge sync protocol. The index is a CRDT mapping
`relative path -> document ID`; the server's periodic sync loop turns each
entry into a filesystem write with **no path containment**:

- `sync_all_documents` (`crates/quarto-hub/src/sync.rs:505`) does
  `project_root.join(file_path_str)` on the untrusted index key, then
  `sync_document` / `sync_binary_document` call `std::fs::write` on the
  result (`sync.rs:151`, `sync.rs:307`).
- A key like `"../../../../home/<svc>/.bashrc"` (or an absolute path)
  escapes `project_root`. The only current guard is the `.exists()` check
  at `sync.rs:508`, so the primitive is **overwrite of any existing file
  the server process can write**, with attacker-controlled content.

This is finding #1 from the 2026-05-30 hub security analysis. There is no
server-side code execution in the relay itself, so this is a file-overwrite
primitive (overwrite-to-RCE is deployment-dependent: shell rc files, configs,
`.git/hooks/*` inside the repo, etc.).

### Threat-model framing

The server cannot distinguish "autosync behaving well" from a modified
client. Containment must hold against **arbitrary bytes on the wire**, which
means the authoritative gate lives at the point where an index-sourced path
becomes a filesystem path — **not** in `IndexDocument::add_file`, which a
malicious client bypasses entirely by mutating the CRDT directly.

### Scope

In scope (the single untrusted index-path -> fs-write site):
- `sync_all_documents` (`sync.rs:491-580`) — **authoritative gate**. This is
  the *only* site that turns an index key into a filesystem path; containing
  it here is both necessary and sufficient.

Verified out of scope (no untrusted index-path -> fs op):
- `sync_file_by_path` (`sync.rs:414`) — path comes from the watcher and is
  already guarded by `strip_prefix(project_root)` (`sync.rs:422`).
- `reconcile_files_with_index` (`context.rs:558-624`) — joins only paths from
  the real filesystem walk (`project_files`), not from the index.
- `list_documents` / `get_document` (`server.rs:485-534`) — return index path
  strings as JSON metadata only; no filesystem read. (A poisoned entry can
  show a bogus path in the listing, but that is cosmetic, not a read/write.)

### Why no defense-in-depth in the index API

`IndexDocument::add_file` / `set_capture` are *not* in scope. A malicious
client bypasses them entirely by mutating the CRDT directly, so validating
there gives zero security value against the actual threat. And the server
never produces an unsafe key itself: both non-test callers (`context.rs:607`,
`context.rs:653`) pass paths from `project_files`, which are already
`strip_prefix(project_root)`'d filesystem-walk paths. Validating `add_file`
would guard a bug that cannot currently occur, at the cost of turning two
infallible-in-practice methods fallible plus their own test matrix. Skip it.

## Design

### Containment helper

A single private function in `sync.rs` — **not** a new module. With the index
API out of scope (see above), `contained_join` has exactly one caller, so a
dedicated module with public functions is more ceremony than the job needs.
The cross-platform `Component` logic still deserves a focused `#[cfg(test)]`
block next to it.

```rust
use std::path::{Component, Path, PathBuf};

/// Join an index-relative path onto `project_root`, rejecting anything that
/// could escape the root. Returns the joined absolute path, or `None` if the
/// relative path is unsafe.
///
/// Rejects: absolute paths, Windows path prefixes (drive / UNC), `RootDir`,
/// and any `..` that pops above the root. Allows ordinary nested paths and
/// `.` components.
fn contained_join(project_root: &Path, rel: &str) -> Option<PathBuf>;
```

**Do not reuse `quarto-core`'s `lexical_clean` (`output_sink.rs:444`).** It
*normalizes* (`..` at the root is kept as a `ParentDir` component) whereas we
need *reject-on-escape*; different semantics, and reaching across a crate
boundary for a ~15-line pure function is not worth the coupling.

Implementation rules (cross-platform — must use `std::path::Component`,
never string `".."` matching, per `.claude/rules/cross-platform.md`):

- Walk `Path::new(rel).components()`.
- `Component::Prefix` (Windows drive/UNC) or `Component::RootDir` -> reject
  (absolute / rooted).
- `Component::ParentDir` (`..`) -> pop one segment; if the stack is already
  empty, reject (escapes root).
- `Component::CurDir` (`.`) -> ignore.
- `Component::Normal(seg)` -> push.
- After walking, **reject if the folded stack is empty** — `a/..`, `.`, or `""`
  all resolve to `project_root` itself (a directory, which `.exists()` would
  pass), and we only ever want a file path strictly *under* the root.
- Otherwise return `project_root.join(<folded relative path>)`.

This is a pure lexical check (no filesystem access), so it is deterministic
and trivially unit-testable on all platforms.

### Symlink-escape check (part of the core fix, not optional)

The lexical check blocks every `..`/absolute/rooted escape, but leaves one
residual hole: a symlink **inside** `project_root` pointing outward, traversed
by a poisoned key with **no** `..` (e.g. key `assets/x` where
`project_root/assets` is a symlink to `/home/user`). The `.exists()` gate
follows the symlink, so the write lands outside the root.

This is reachable by **the same threat actor as the main fix** — a malicious
wire client — and does *not* require a separate write primitive:

- The attacker need not plant the symlink. *Pre-existing benign* symlinks are
  common (`node_modules`, symlinked asset/content dirs, build outputs); the
  client merely names a key that traverses one.
- Discovery uses `WalkDir::follow_links(false)` (`discovery.rs:42`), so the
  legitimate index never contains keys that traverse symlinked dirs. The only
  source of such a key is a malicious client — so this check has **no
  legitimate false positives**; it rejects exactly the attack.

Because `.exists()` already guarantees the target exists by the time we'd
check, the implementation is a plain `canonicalize` + `starts_with` — **not**
the `canonicalize_deepest_existing` helper at `output_sink.rs:420` (that one
exists for not-yet-created paths; unnecessary here). Canonicalize
`project_root` **once before the loop** (it is loop-invariant); only the
per-entry target is canonicalized inside the loop:

```rust
// once, before the loop:
let real_root = std::fs::canonicalize(project_root).ok();

// per entry, immediately before the synchronous write unit (see Wiring):
let real = std::fs::canonicalize(&file_path).ok();
// reject (count + warn + continue) unless real & real_root are both Some
// and real.starts_with(real_root)
```

**Fail-closed on root-canonicalize failure (intended).** If
`canonicalize(project_root)` returns `None`, `real_root` is absent and the
per-entry rule rejects *every* file — the sync writes nothing this cycle. That
is the correct fail-closed posture, but the per-entry `warn!` is an
attack-signal log, so N rejections from a transient root problem would read as N
attacks. Emit **one** distinct `error!` before the loop when root
canonicalization fails (e.g. "project root not canonicalizable, rejecting all
index writes this cycle") and skip the loop, rather than letting the per-entry
warns flood. In practice discovery has just walked the root, so this should
never fire — but the log must not be misleading if it does.

Canonicalizing *both* sides also resolves OS aliases (macOS `/var`→`/private/var`)
consistently. Native-only (`std::fs::canonicalize` is unavailable on WASM, and
quarto-hub is a native server binary anyway).

#### TOCTOU: minimized, not eliminated — and why that is correct here

`canonicalize` resolves the path, then the write happens afterward; a symlink
swapped in that window would defeat the check. We make the window as small as
possible — and explain why closing it entirely is neither necessary nor worth
the cost.

**Minimization (do this).** Place the `canonicalize` check **immediately before
the `sync_document_auto` call** (i.e. *after* the `repo.find(...).await`), not
up next to `contained_join`. The sync loop is single-threaded and there is no
`.await` between the check and the write, so no other task on this runtime can
interleave; the only remaining gap is the synchronous read-merge-write inside
`sync_document_auto`, which no cooperative scheduler yield can split. That is
the tightest window achievable without restructuring the merge to fd-based I/O.

**Why not eliminate it fully.** True TOCTOU-freedom requires either Linux-only
`openat2(RESOLVE_BENEATH)` or a portable component-by-component
`openat(O_NOFOLLOW)` walk from a `project_root` fd with the merged write
threaded through that descriptor — both invasive, and the portable form is a
userspace reimplementation of kernel path resolution. More importantly it buys
nothing against this fix's threat actor: exploiting the window requires
swapping a symlink **inside `project_root` on the server's filesystem**, which
needs local fs write access. Any actor with that access can already write
wherever the server process can write and does not need this primitive at all.
The wire-client threat (arbitrary bytes over the sync protocol, no local fs
access) cannot reach the window. So we minimize and document; we do not pursue
`openat2`.

### Wiring into `sync_all_documents`

Replace `let file_path = project_root.join(file_path_str);` (`sync.rs:505`)
with a `contained_join` call. On rejection: `warn!` with the offending path
(attack signal), increment a new `rejected: usize` counter on `SyncAllResult`,
and `continue` (do not touch the filesystem).

Both downstream `std::fs::write` sites — `sync.rs:151` (text) and `sync.rs:307`
(binary) — receive their `file_path` from this function via
`sync_document_auto` (`sync.rs:564`), so containing the single join here
contains both writes; no per-writer change is needed.

The lexical `contained_join` replaces the join at line 505 (reject early,
before `.exists()`). The `canonicalize` symlink check goes **immediately before
`sync_document_auto`** (after the `repo.find(...).await`) to keep the TOCTOU
window minimal (see above) — same reject path: count + `warn!` + `continue`.

`sync_all_documents` only receives `repo / index / project_root / sync_state`
— **no peer/email is in scope here**, so the `warn!` is path-only. The peer
identity is already captured by the access log at the connection layer; do not
plumb it down just for this log line.

`SyncAllResult` (`sync.rs:601-620`) gains `rejected: usize`. `total_synced`
and `has_changes` are unaffected by it (rejected entries are neither synced
nor changes). Update the summary log line to surface `rejected` when > 0.

**Rejection is per-sync-cycle, by design.** The gate rejects the *write* but
leaves the poisoned key in the CRDT index — we do not mutate client-owned index
state from the server. So a single poisoning re-rejects (and re-`warn!`s) on
every sync interval until a client removes the key. Expected behavior, not a
leak: a recurring rejection warn for the same path is one stale poisoned entry,
not a repeated live attack. Cleaning poisoned keys out of the index is a
separate concern (and a client action), out of scope for this fix.

## Phase 1 — Tests first (write, run, watch them fail)

- [x] Unit tests for `contained_join`:
  - [x] accepts `index.qmd`, `chapters/intro.qmd`, `a/./b.qmd`
  - [x] accepts internal `..` that stays in root: `a/../b.qmd` -> `b.qmd`
  - [x] rejects `../escape.txt`, `a/../../escape.txt`
  - [x] rejects absolute unix `/etc/passwd`
  - [x] rejects rooted / Windows-prefixed paths (`\\?\C:\x`, `C:\x`, `/x`)
        using `Component` semantics (cross-platform assertions)
  - [x] rejects paths that fold to the root itself: `""`, `.`, `a/..`,
        `a/b/../..` (all resolve to `project_root`, not a file under it)
- [x] Regression test driving the **real consumption path** (per CLAUDE.md
      end-to-end guidance — route through `HubContext::sync_all()`
      (`context.rs:337`), not the bare function):
  - [x] Build a temp project root with a legitimate `index.qmd`.
  - [x] Create a victim file *outside* the root (sibling temp dir) so
        `.exists()` would pass.
  - [x] Poison the index with `"../<victim>" -> <doc_id>` and create that doc
        with attacker content (mirror the existing `sync.rs:945-1007` test
        setup that calls `index.add_file` + `repo.create`). Use
        `index.add_file("../<victim>", doc_id)` directly: by design `add_file`
        performs **no** containment (see "Why no defense-in-depth"), so it
        produces the identical poisoned CRDT state a malicious wire client
        would — no need to drop to a raw transaction. Add a one-line comment
        noting this is a faithful attack model *because* `add_file` is
        unvalidated.
  - [x] Run `sync_all` / `sync_all_documents`; assert the victim file is
        **unchanged**, `result.rejected == 1`, and the legitimate file still
        syncs.
- [x] Symlink-escape regression test (`#[cfg(unix)]` — `std::os::unix::fs::symlink`,
      per cross-platform rule): create `project_root/assets` as a symlink to a
      sibling temp dir holding a victim file; poison the index with key
      `assets/<victim>` (no `..`, passes the lexical check); assert the victim
      is **unchanged** and `result.rejected == 1`.
  - [x] Note: the runtime `canonicalize` + `starts_with` check itself runs on
        all platforms, but this regression exercises it on **Unix only**
        (Windows symlink creation needs privilege; the cross-platform rule gates
        symlink tests to `#[cfg(unix)]`). The Windows behavior of the check is
        unverified by tests — acceptable, but state it rather than imply parity.
- [x] Run targeted tests, confirm the regression + traversal tests FAIL
      before implementation (`cargo nextest run -p quarto-hub`).

## Phase 2 — Implement containment

- [x] Add a private `contained_join` fn (+ `#[cfg(test)]` unit block) in
      `sync.rs`; no new module.
- [x] Wire `contained_join` into `sync_all_documents` (`sync.rs:504-516`);
      add `rejected` to `SyncAllResult` + `warn!` on rejection.
- [x] Add the `canonicalize` + `starts_with` symlink-escape check **immediately
      before `sync_document_auto`** (after the `repo.find(...).await`), with
      `canonicalize(project_root)` hoisted **once above the loop**. Same reject
      path: count + `warn!` + `continue`. Plain `canonicalize`, not
      `canonicalize_deepest_existing` — `.exists()` already guarantees the
      target exists. This placement minimizes the TOCTOU window (no `.await`
      between check and write); see the TOCTOU subsection for why full
      elimination is unnecessary.
  - [x] When the hoisted `canonicalize(project_root)` is `None`, emit one
        distinct `error!` and skip the loop (fail-closed) — do **not** fall into
        the per-entry attack-signal `warn!` N times. See the fail-closed note in
        the symlink-escape design section.
- [x] Re-run Phase 1 tests; confirm traversal + lexical + symlink + regression
      tests now PASS.

## Phase 3 — Verification

- [x] `cargo nextest run -p quarto-hub` green.
- [x] `cargo nextest run --workspace` (no regressions in dependents).
- [x] `cargo xtask verify --skip-hub-build` — quarto-hub is a server binary;
      `wasm-quarto-hub-client` does NOT depend on it, so the WASM leg is
      unaffected and `--skip-hub-build` is the correct level here.
- [x] End-to-end note: record the exact test invocation + the assertion that
      the outside-root victim file was not overwritten. Full client-driven
      browser/autosync e2e is optional/manual; the `sync_all`-routed
      regression test is the in-repo end-to-end gate.

## Verification record (2026-05-31)

End-to-end gate is the `sync_all`-routed regression test
(`context::tests::sync_all_rejects_parent_traversal_write`), driven via
`HubContext::sync_all()` — the real consumption path:

```
cargo nextest run -p quarto-hub sync_all_rejects
```

Both regression tests pass; each asserts the outside-root victim file's
content is **unchanged** (`"original"`) and `result.rejected == 1` while the
legitimate `index.qmd` still syncs. `cargo nextest run --workspace` (9491
tests) and `cargo xtask verify --skip-hub-build` both green.

## Notes / decisions

- This fix is independent of the auth and authorization findings (#2-#4 from
  the security analysis); it holds even with auth disabled, which is why it is
  the highest-leverage single change.
- Keep code comments terse one-liners (user preference).
