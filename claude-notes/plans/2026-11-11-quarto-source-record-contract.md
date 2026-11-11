---
title: "`.quarto-source.yml` as a shared contract: unknown keys, schema version, spec, track/commit"
strand: bd-yjg91sls
issues: [1175, 1176, 1177, 1178]
status: Phases 1–2 + CRLF/.gitattributes implemented (PR pending); Phases 3–4 for discussion
date: 2026-11-11
---

# `.quarto-source.yml` as a shared contract

Four issues from mcanouil (Quarto Wizard maintainer), filed together. Quarto Wizard's
dev version now writes `.quarto-source.yml` in q2's format so an extension installed by
one tool can be updated by the other. That turns an internal file of `q2 add`
(bd-rfz4e3bw, plan `2026-10-22-q2-add-extension.md`) into an interchange format, and
exposes the gaps:

| Issue | Ask | Kind | Proposed disposition |
|---|---|---|---|
| #1175 | Rewriting the record drops keys q2 doesn't know | bug | **Fix now** (Phase 1) |
| #1176 | A record with `version: 2` is read under v1 rules and downgraded | bug | Fix now, small design choice (Phase 2) |
| #1177 | Publish a spec + JSON Schema for the record | docs/design | Discuss (Phase 3) |
| #1178 | Add optional `track` and `commit` keys | feature | Discuss (Phase 4) |

## Where the code is

All in `crates/quarto/src/commands/add/`:

- `record.rs` — `SourceRecord { version, source, kind, ref?, files }`, `RECORD_VERSION = 1`,
  `installed_state(dir)` (parses the old record, re-hashes, returns `InstalledState`
  only — the parsed record is discarded), `hash_tree` / `list_files` / `slash_path`.
- `install.rs` — `write_record(plan, staged)` builds a **fresh** `SourceRecord` from
  `plan.provenance` + `hash_tree(staged)`. Per-plan, not per-extension. Also `copy_tree`
  drops any record the *source* shipped (correct; keep).
- `resolve.rs` — builds `Provenance { source, kind, git_ref }`; `git_ref` is the text
  after the last `@` of what the user typed. `PlannedExtension.installed` holds the
  `InstalledState`. `refuse_local_edits` turns `Modified(_)` into the `--force` refusal.
- `crates/quarto-source-fetch/src/target.rs` — GitHub candidates: bare `org/repo` →
  `refs/heads/main`, `refs/heads/master`; `@ref` → `refs/tags/<r>`, `refs/heads/<r>`,
  `archive/<r>` (commit; bd-pr8yrfm1 / #1005). `fetch.rs::fetch_first_available` returns
  `()` — **which candidate won is thrown away**.
- `crates/quarto-source-fetch/src/archive.rs:379` — the tar extractor already recognises
  GitHub's `pax_global_header` "holding the commit id" and **skips it**.
- `q2 update` is a `NotImplemented` stub (`commands/update.rs`).
- User docs: one bullet in `docs/guides/authoring/extensions.qmd:55`.

## Phase 1 — #1175: preserve unknown keys (fix now)

Repro confirmed by reading: `installed_state` parses then drops the record;
`write_record` constructs from provenance only.

Change:

1. `SourceRecord` gains `#[serde(flatten)] extra: BTreeMap<String, serde_yaml::Value>`.
   Known keys are q2's; everything else round-trips. (Serialized order: q2's keys, then
   extras in sorted order. Comments in the old file are not preserved — the header is
   regenerated. Fine: it is a machine-owned file.)
2. `installed_state` returns the parsed record alongside the state (e.g.
   `InstalledState::{Unmodified, Modified}` carry `Option<SourceRecord>`, or a separate
   `prior_record` field on `PlannedExtension` — prefer the latter; keeps `InstalledState`
   `Eq`-comparable in the existing tests).
3. `write_record` becomes per-extension: owned fields from the plan, `extra` copied from
   the prior record when there is one.
4. Tests (unit, `record.rs` + an install-level test): unknown top-level scalar and map keys
   survive an Update; an unknown key does **not** make the install count as edited (the
   record is excluded from hashing already — assert it); a source-shipped record's keys are
   *not* carried (copy_tree already drops it — assert nothing leaks from source to dest).

Open sub-questions (minor; my defaults in bold):

- With `--force` over an *unreadable* record there is nothing to carry. **Drop.**
- Do we carry extras on a **Revert** / reinstall under `--force` with `[No Change]`? **Yes,
  always carry when the prior record parses** — q2 doesn't know what they mean, so it
  can't judge them stale.
- Staleness: a third-party key that describes the *old* install (Wizard's
  `quarto-wizard.commit`) is now wrong after q2 replaced the files. That is inherent to
  "keep what you don't understand"; the spec (Phase 3) should say tools must treat their
  own keys as possibly stale when `files` no longer matches, or namespace them and
  re-validate. Phase 4 also removes the main motivating case by giving `commit` a
  q2-owned key.

## Phase 2 — #1176: refuse a newer schema version

Today `version` is deserialized into `u32` and never compared. A v2 record that happens to
parse under v1 shapes gets its hashes checked with v1 meaning, then is rewritten as v1.

Change: parse in two steps. First read the YAML into a `Value` and pull `version`
(missing / non-integer → existing `UnreadableRecord`). If `version > RECORD_VERSION` →
a new `EditKind::NewerRecord(u32)` rendered as
`.quarto-source.yml (written by a newer q2: record schema version 2, this q2 reads 1)`,
going through the existing `--force` refusal. Only then deserialize as `SourceRecord`.
The probe-first order matters: a genuine v2 record may *not* parse as v1, and should get
the "newer" message, not "unreadable".

Design choice for you: reuse the **local-changes refusal** (`--force` overrides; simplest,
matches the issue's expected behaviour), or a **distinct error** that suggests upgrading
q2 and is still `--force`-overridable. I lean toward the first with the distinct line
text above, plus a hint line in the error body when any `NewerRecord` is present
("Upgrade q2, or re-run with --force to replace it with a version 1 record").

Also: `version: 0` / negative → `UnreadableRecord`. Does `--force` over a newer record
carry its unknown keys (Phase 1)? **No** — we can't know which of its keys are "owned"
under v2 semantics; write a clean v1 record.

## Phase 3 — #1177: publish the contract (discuss)

What a spec has to pin down, with the current behaviour and questions it surfaces:

1. **Fields.** `version` (int, schema version), `source` (string as typed — includes any
   `@ref`, so `ref` is redundant with it today), `kind` (`github | url | local-archive |
   local-dir`), `ref` (optional), `files` (map path → lowercase hex sha256). Plus Phase 4
   keys if accepted.
2. **Version policy.** Proposal: additive optional keys do **not** bump `version`
   (that's what makes #1178 possible in v1); a bump means "a reader at an older version
   would misinterpret something" (e.g. hash algorithm or path-normalization change).
   Readers that see a higher version refuse to verify/overwrite without explicit consent
   (Phase 2).
3. **Unknown keys** are preserved verbatim on rewrite (Phase 1). Recommend third-party
   keys be namespaced under a single top-level key per tool (`quarto-wizard:`), and q2
   reserves all un-namespaced top-level keys for itself. (Question: do we want an explicit
   reserved prefix like `x-…` instead? I'd rather "one top-level map per tool".)
4. **Hash rules.** sha256 of raw on-disk bytes; keys are `/`-joined paths relative to the
   extension dir; symlinks followed; only regular files; the top-level
   `.quarto-source.yml` excluded (a nested one is *not* excluded — intended?).
   **Two real problems surface when writing this down:**
   - **CRLF.** A record committed from macOS/Linux, checked out on Windows with
     `core.autocrlf=true` (Git for Windows' install default), re-hashes every text file
     differently → every file "modified" → `q2 add` refuses without `--force`.
     **Decided (2026-11-11): keep raw-byte hashing; fix it with `.gitattributes`.** See
     § CRLF: the `.gitattributes` design below.
   - **OS junk files.** `.DS_Store`, `Thumbs.db`, editor swap files inside an extension
     dir count as `Added` → local-edit refusal. Do we exclude a fixed ignore list? Again
     cheaper to decide before third parties implement the hash rule.
5. **Where it lives.** Proposal: a reference page `docs/guides/extensions/source-record.qmd`
   (user-facing, shipped with the docs site), and a JSON Schema at
   `crates/quarto/schemas/quarto-source-record.v1.schema.json` (following
   `crates/quarto-core/schemas/pandoc-request.schema.json`), also copied/served from the
   docs site at a stable URL so Wizard can `$ref` it. A test validates q2's own output
   against the schema (needs a JSON-Schema validator dev-dependency — check the
   workspace; none today) so they cannot drift.
6. **Stability promise.** Who is the audience — "other tools may rely on this" means
   we're committing to it pre-1.0. Worth deciding explicitly.

### CRLF: the `.gitattributes` design

**Precedent.**

- *Quarto 1* has no `.gitattributes` handling anywhere. The nearest thing is
  `src/project/project-gitignore.ts` (`ensureGitignore`), which appends missing entries to
  the **project-root** `.gitignore`, and creates one only when `git rev-parse` says we're in
  a repo.
- *q2 product code* uses the opposite pattern: it writes a **self-contained file inside a
  directory the tool owns**, only if absent, and never edits the user's root files.
  `.quarto/preview-out/.gitignore` = `*` (`quarto-core/src/preview_out.rs:67`); the hub dir's
  catch-all `.gitignore` (`quarto-hub/src/storage.rs:525`, bd-sekcpmv1).
- *q2 repo itself* pins byte-compared/hashed fixtures in the root `.gitattributes`, using
  `-text` where bytes must be exact (`crates/quarto-core/tests/fixtures/units/* -text`).
- *Similar tools* hash vendored working-tree files and hit exactly this bug, and none of them
  fix it in the tool: `cargo vendor`\'s `.cargo-checksum.json` (users.rust-lang.org threads),
  `go.sum` (golang/go#33178, still open on whether `go` should override the local setting),
  and R `md5sum` users. In every case the advice is "turn off autocrlf, or add a
  `.gitattributes` rule". Git LFS is the precedent for a tool that writes `.gitattributes`
  itself (`git lfs track`).

**Proposal.**

1. `q2 add` writes **`_extensions/.gitattributes`** containing a header comment and
   `* -text`, **only if the file is absent**. It never edits an existing one and never touches
   the root `.gitattributes`. This follows the q2 "own file in an owned directory" pattern.
   - It sits *outside* every extension directory, so it is never hashed, never collides with
     a `.gitattributes` the source ships inside its extension folder, and isn't removed
     when an extension is replaced.
   - A pattern without a `/` matches at every depth below the file's directory, and a deeper
     `.gitattributes` overrides the root one. So this beats a user's `* text=auto`.
   - **`-text`, not `text eol=lf`.** The forum advice (`* text=auto eol=lf`) is wrong for us.
     `eol=lf` normalizes on commit, so an extension that *ships* CRLF files (authored on
     Windows) would be committed as LF, and its recorded hashes would stop matching on every
     platform. `-text` means "never convert in either direction", so the bytes git stores and
     checks out are the bytes `q2 add` hashed. Diffs still work (`-text` ≠ `binary`).
   - Written unconditionally, not only inside a git repo. It's inert elsewhere, and a project
     often becomes a repo after the extension is added. The file has to exist **before the
     first commit** of the extension, because a commit made under autocrlf has already
     normalized the blobs. That's why docs alone aren't enough.
   - `--dry-run` reports it would be written. The `--json` result gains a
     `gitattributes_written` field. The human footer gets one line: `Created
     _extensions/.gitattributes, so git keeps extension files byte-for-byte (…)`.
2. **Spec text (#1177):** hashes are of raw bytes. Tools that write the record SHOULD
   ensure `_extensions/.gitattributes` with `* -text` (same only-if-absent rule).
3. **Diagnostic for the already-broken case** (repos committed before this ships, or a user
   who deleted the file): in `installed_state`, when a file's hash mismatches, also hash it
   with CRLF→LF. If *that* matches the record, report a distinct
   `EditKind::LineEndings` ("line endings changed, probably by git's core.autocrlf"),
   with a hint pointing at the `.gitattributes` fix. Cheap, and it turns a baffling
   "every file modified" into an actionable message.

**Resolved (2026-11-11, with Carlos).**

- An EOL-only difference **still needs `--force`**, and the error carries the LineEndings
  hint. The hash rule stays exact, so other tools don't have to implement a fuzzy
  normalization rule.
- If `_extensions/.gitattributes` exists but lacks a `-text` rule, `q2 add` **does not
  warn**. User-owned files are left alone, and the LineEndings diagnostic catches the cases
  that actually break.
- A `[No Change]` run **does not write** the file. "No Change writes nothing" is a
  documented promise.

**Sources consulted (2026-11-11).** We kept these so the reasoning survives link rot. Each
entry says what the source showed and how it fed the decision. The external ones were
read through web-search summaries, not studied in full. They were checked for the
pattern (tools hash working-tree files, autocrlf breaks the hash, and the fix is left to
the user), not for every detail.

- *Rust forum, "Issues with vendored packages changing checksums without any (manual)
  changes"*
  (<https://users.rust-lang.org/t/issues-with-vendored-packages-changing-checksums-without-any-manual-changes/117783>).
  A Windows user cloned a repo made on Linux, and `cargo vendor`\'s `.cargo-checksum.json`
  (per-file sha256 of the vendored tree, the closest analogue to our record) rejected a
  vendored file. The cause was git's autocrlf conversion. The fixes suggested were
  `core.autocrlf=false` or a `.gitattributes` rule (`* text=auto eol=lf`). **Takeaway:**
  the exact failure we predicted, in a mature tool, and Cargo does nothing about it. The
  suggested `eol=lf` rule is *not* what we want (see "`-text`, not `text eol=lf`" above).
- *Rust forum, "Cargo and line endings on Windows"*
  (<https://users.rust-lang.org/t/cargo-and-line-endings-on-windows/94380>). Notes that
  **Git for Windows sets `core.autocrlf=true` by default at install**, and suggests
  `*.rs text eol=lf` in `.gitattributes`. **Takeaway:** this is the default Windows
  experience, not an edge case, so a fix that ships only in the docs isn't enough.
- *golang/go#33178* (<https://golang.org/issue/33178>, mirror
  <https://github.com/golang/go/issues/33178>). Module checksum verification
  failed on Windows but passed on Linux, because autocrlf changed the bytes. The issue
  discusses whether the `go` tool should override the user's local git setting, and was
  still open as of our search. **Takeaway:** the Go team also could not settle on fixing
  it inside the tool. That supports keeping the hash rule exact and handling it at the
  git-attribute layer.
- *R-help, "md5sum issues" thread, Feb 2021*
  (<https://stat.ethz.ch/pipermail/r-help/2021-February/470167.html>). R users comparing
  `md5sum` values across platforms were told to set `git config --global core.autocrlf
  false`. **Takeaway:** the same problem reaches the R/Quarto user base. It's a third
  ecosystem with the same story.
- *GStreamer issue #1754, "Incorrect line endings in the Windows CI image checkout"*
  (<https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/1754>). Changing the
  autocrlf setting only took effect after a fresh clone. **Takeaway:** files committed
  or checked out before the attribute exists stay broken. That's why the file must be
  written at install time, before the first commit, and why the LineEndings diagnostic is
  needed for repos that already broke.
- *Git LFS (`git lfs track`)*: general knowledge, not looked up again. It's the standard
  example of a tool that writes `.gitattributes` on the user's behalf. **Takeaway:**
  a tool writing `.gitattributes` is an accepted practice.
- *Quarto 1*: `external-sources/quarto-cli/src/project/project-gitignore.ts`
  (`ensureGitignore`). This is the only VCS-hygiene file Q1 manages. It appends missing
  entries to the project-root `.gitignore`, creates one only when `git rev-parse` succeeds,
  and writes CRLF on Windows. A grep of `external-sources/quarto-cli/src` found no
  `gitattributes` references. **Takeaway:** there's no Q1 precedent to match. We chose not
  to copy its "edit the root file" approach.
- *q2 product code*: `crates/quarto-core/src/preview_out.rs:67` (writes
  `.quarto/preview-out/.gitignore` = `*` if absent) and `crates/quarto-hub/src/storage.rs:525`
  (hub dir's catch-all `.gitignore`, bd-sekcpmv1). **Takeaway:** this is the pattern we
  follow: a self-contained file in a directory the tool owns, written only if absent,
  with user root files never edited.
- *q2 repo `.gitattributes`*: this repo already pins hashed or byte-compared fixtures, and
  uses `-text` where bytes must be exact (`crates/quarto-core/tests/fixtures/units/* -text`,
  bd-1d6io for the annotated-qmd fixtures). **Takeaway:** we already rely on `-text`
  ourselves for the same reason.

## Phase 4 — #1178: `track` and `commit` (discuss)

The facts are cheaper to get than the issue assumes:

- **`commit` is free for every GitHub install.** GitHub tarballs (made by `git archive`)
  carry the commit SHA in the pax global header (`comment=<sha>`); `archive.rs` already
  sees that header and skips it. Parse it instead and surface it in `ExtractSummary` /
  `FetchedSource` (GitHub zips carry it as the archive comment, if we ever fetch zips). No
  GitHub API call, no auth, no rate limits — and it covers bare `org/repo` and `@branch`
  installs, not just `@<sha>`.
- **`track` falls out of which candidate URL won.** `refs/tags/…` → tag, `refs/heads/…`
  (including the bare main/master probe) → branch, `archive/<sha>` → commit.
  `fetch_first_available` just needs to return the winning candidate (or its kind).
  `quarto-source-fetch` is shared with `q2 use brand`; additive API change.

Questions for you:

1. **Enum values.** The issue proposes `tag | branch`. An `@<sha>` install follows
   nothing; I'd add **`commit`** (pinned) rather than omit `track`, so "absent" keeps
   meaning "unknown / not GitHub".
2. **What does `track: tag` mean to `q2 update`?** "Newest release", "newest semver tag",
   or "stay on this tag"? Recording the fact (`tag`) is uncontroversial; the update
   semantics belong to the `q2 update` design. Suggest the spec only says what was
   installed, and defers "what an update does" to `q2 update`.
3. **`kind: url` / local.** No `track`; `commit` only if a tarball happens to carry a pax
   commit (a GitHub archive URL typed in full does — record it? I'd say yes, it's a fact
   about the bytes).
4. **Keep `ref`?** It's what the user typed; `source` already contains it. Keep for
   compatibility; spec documents it as informational.
5. **Additive under v1** (agree with the issue) — consistent with the Phase 3 version
   policy, and Phase 1 guarantees older q2s won't strip the keys once they have the fix.
   Older *released* q2 (≤0.43) will strip them — acceptable.

## Progress

- [x] Phase 1 (#1175): `SourceRecord.extra` (`#[serde(flatten)] serde_yaml::Mapping`, keeps
  file order); `record::inspect` returns the parsed prior record; `write_record` carries
  its `extra`. Not carried from an unreadable or newer-version record.
- [x] Phase 2 (#1176): `parse_record` probes `version` first; `EditKind::NewerRecord(v)`
  goes through the `--force` refusal with an "Upgrade q2" hint. `version` 0 / missing /
  non-integer → `UnreadableRecord`.
- [x] CRLF: `_extensions/.gitattributes` (`* -text`) created when missing by any run that
  writes an extension (not `[No Change]`, not `--dry-run`, which reports "would also
  create"); `--json` gains `gitattributes_written`. `EditKind::LineEndings` (CRLF↔LF in
  either direction) with the autocrlf hint. Checked by hand in a real repo:
  `git check-attr` shows `text: unset` under a root `* text=auto`, and an LF file checks out
  as LF with `core.autocrlf=true`.
- [x] Docs: `docs/guides/authoring/extensions.qmd` bullets.
- [ ] Phase 3 spec + schema (#1177): remaining questions are namespacing, the junk-files
  ignore list, and where the schema lives.
- [ ] Phase 4 `track` / `commit` (#1178).

## Suggested sequencing

1. Phase 1 + Phase 2 as one PR (both touch `installed_state` and `write_record`; both
   small). Close #1175, #1176.
2. Settle Phase 3 questions (CRLF, junk files, namespacing, schema location) — the hash
   rule decisions are the ones that are expensive to change later.
3. Phase 4 (source-fetch returns winning candidate + pax commit; record writes
   `track`/`commit`) together with the spec text, so the spec documents the final v1
   shape once. Close #1177, #1178.

## Related

- bd-rfz4e3bw — original `q2 add` implementation (closed).
- bd-pr8yrfm1 / #1005 / PR #1006 — commit-SHA candidate URL (in progress).
- `q2 update` — unimplemented; Phase 4 is its main prerequisite.
