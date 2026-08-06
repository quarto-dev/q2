# Glob semantics — one meaning for every pattern in q2

**Status:** in progress under bd-mt7a6uc4. The API lives in
`crates/quarto-core/src/glob/`. Listing `contents:` and `project.render` are
migrated; `resources:` and `sidebar.auto:` follow in later phases of that strand
(see `claude-notes/plans/2026-08-06-glob-consumer-migration.md`).

**Audience:** anyone adding a config key that accepts user-written path patterns,
and anyone changing what an existing pattern matches.

**The rule in one line:** a glob means the same thing everywhere in q2, and that
meaning is defined here — not in the consumer.

---

## Why this exists

Four subsystems accept globs: listing `contents:`, `project.render`,
`resources:`, and `sidebar.auto:`. Each grew its own matcher, its own
base-directory rule, and its own failure modes. The result was that the *same
string* meant four different things — `docs/*.qmd` matched nested files in a
sidebar but not in a listing; `[0-9]` was a character class in `resources:` and
three literal characters everywhere else; a leading `/` meant the project root
in one place, was silently dropped in another, and matched nothing in a third.

None of that was a decision. It was four independent implementations drifting.

---

## The normative semantics

### Pattern vocabulary

Every consumer gets the full vocabulary of the `glob` crate, matched with
[`MATCH_OPTIONS`](../../crates/quarto-core/src/glob/matcher.rs):

| Syntax | Meaning |
|---|---|
| `*` | any run of characters **within one path segment** |
| `?` | exactly one character, never `/` |
| `**` | zero or more whole path segments; must be a complete segment (`a**b` is an error) |
| `[abc]`, `[a-z]` | character class |
| `[!abc]` | negated character class |
| `[*]`, `[?]` | a literal `*` / `?` |

Matching is **case-sensitive on every platform**. Windows filesystems are
case-insensitive, but the pattern-to-path comparison is ours to define, and a
project that renders differently on macOS than on Linux is a worse outcome than
one that asks the author to match case.

`*` matches leading dots: `data/*` includes `data/.nojekyll`. Dotfiles are
excluded — where they are excluded at all — by the *discovery* layer, not by
pattern semantics.

### Anchoring

| Form | Resolves against |
|---|---|
| `posts/*.qmd` | the directory of the file the pattern was **written in** |
| `/posts/*.qmd` | the **project root**, always |
| `../posts/*.qmd` | the declaring file's directory, one level up — clamped at the project root |

"The file it was written in" means: document front matter → the host document's
directory; `blog/_metadata.yml` → `blog/`; `_quarto.yml` → the project root. It
is recovered from the value's `SourceInfo` provenance, so it keeps working
through metadata merging. Values with no recoverable file (runtime `--metadata`,
programmatic config) fall back to a caller-supplied directory.

A pattern whose `..` segments climb above the project root **matches nothing**
and is reported.

### Negation

A leading `!` marks an exclusion. An item matches iff it matches at least one
positive pattern and no negative one. Order is irrelevant: moving the `!` entry
does not change the result. A `!` anywhere other than position 0 is an ordinary
filename character.

### Directories

A pattern with no metacharacters that names a directory matches everything
beneath it: `posts` and `posts/` both match `posts/welcome/index.qmd`. Note that
`[` counts as a metacharacter — `data/[0-9]` is a class, not a directory name.

This is a per-consumer option (`GlobOptions::directory_rule`), on for every
consumer today, because "this literal string is a directory prefix" is a policy
choice rather than a property of glob syntax.

### Failure modes

Three ways a pattern can fail, all reported rather than silently dropped:

| Failure | Where it lands | Example |
|---|---|---|
| escapes the project root | `GlobResolution::escaped` | `../../*.qmd` at the root |
| the glob engine rejects it | `GlobResolution::invalid` | `a**b.qmd`, `data/[.csv` |
| compiles and matches nothing | consumer's "matched nothing" diagnostic | a typo |

Each carries the `SourceInfo` of the YAML scalar it came from, so the diagnostic
points at what the author wrote.

---

## Deliberate divergences from Quarto 1

Q1 routes `project.render` and `resources:` through `resolveGlobs`
(`external-sources/quarto-cli/src/core/path.ts:227`). Two of its behaviors we do
**not** copy:

1. **No implicit `**​/` prefix.** In Q1's "smart" mode any pattern not anchored
   with `/` is silently rewritten to `**/<pattern>`, so `*.qmd` means "anywhere
   in the tree". In q2, `*` is one segment and you write `**/` when you mean it.
   Q1's rule makes the common case surprising and is applied inconsistently
   across Q1's own code paths — it is the "`*.qmd` vs `**/*.qmd`" inconsistency
   this design exists to end.
2. **No filesystem probing to decide what a pattern means.** Q1 calls `statSync`
   to decide whether a literal is a directory. q2's expansion decides
   structurally, so pattern meaning does not depend on what happens to exist on
   disk at resolve time — and so the same code runs in the browser.

A Q1 project relying on either will render differently under q2. That is
accepted (q2 is 0.x); the "matched nothing" diagnostic is the migration aid.

---

## Architecture: matching is separable from walking

The `glob` crate ships two independent halves:

- `glob::glob()` — a filesystem walker. **Not used.** It is the only part that
  touches `std::fs`.
- `glob::Pattern::matches_with` — a pure string matcher. **This is what q2
  uses.**

Enumeration is the caller's job, and that is what makes one implementation serve
every target:

| Consumer | Candidate source |
|---|---|
| listing `contents:` | the in-memory `ProjectIndex` — no I/O at all |
| `project.render` | a `SystemRuntime` walk of the project's `.qmd` files |
| `resources:` | a `SystemRuntime` walk (any extension) |
| `sidebar.auto:` | the in-memory `ProjectIndex` |

`SystemRuntime` is implemented by both `NativeRuntime` (real filesystem) and
`WasmRuntime` (the hub-client automerge VFS), so `resources:` expansion works in
the browser. **Nothing under `crates/quarto-core/src/glob/` may call `std::fs`
directly** — that is the invariant that keeps this true.

---

## Adding a new glob consumer

1. Collect the raw patterns **with their `SourceInfo`**. Without provenance you
   cannot resolve the base directory or place a diagnostic.
2. Pick a `GlobOptions`. If you need a knob that does not exist, add it here
   first with the justification — a knob nobody can defend in this document is a
   bug, not a feature.
3. Call `resolve_patterns`, then report `escaped` and `invalid` against each
   pattern's span. Do not drop them silently; that is the failure mode this
   whole design replaces.
4. Compile once with `PatternSet::compile` and match in a loop. Do not compile
   per candidate.
5. Add your consumer to the options table in
   `crates/quarto-core/src/glob/mod.rs` tests, so a future divergence shows up as
   a test diff.

### What does *not* belong in `GlobOptions`

Discovery policy. The set of paths a subsystem refuses to look at — `_`-prefixed
components, hidden files, `node_modules`, the output directory, `README` — is a
property of *that subsystem's enumeration*, not of glob semantics. `resources:`
deliberately publishes `.nojekyll` and `_data/x.csv`; `project.render`
deliberately skips them. Keep those rules in the enumerator.
