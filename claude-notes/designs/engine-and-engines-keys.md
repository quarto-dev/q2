# The `engine:` and `engines:` metadata keys (design contract)

**Status:** design contract — authoritative for what each key means and,
just as importantly, what each key is guaranteed *not* to do. Describes the
settled behavior delivered by the **TS Engines Epic**; § History records
how the two keys reached it. Referenced by that epic's implementation
plans.
**Created:** 2026-07-06 (during the TS Engines Epic's engine-key design
work).
**Companion contract:** `engine-resolution.md` owns the resolution
algorithm these keys feed (claim kinds, tiers, ownership); this document
owns the *user-facing grammar* — which key to write, and why.

---

## 1. Why there are two keys

Quarto has always had two engine-related configuration keys, and they have
always been easy to confuse — one letter apart, both about engines. The
q2 design resolves the confusion by giving each key exactly one question
to answer:

- **`engine:`** answers *"which engines are at play in this document?"*
- **`engines:`** answers *"how do engines behave, whichever ones end up
  at play?"*

Everything else in this document is a consequence of that division. When
you are deciding which key to write, ask which question you are answering:
if you are picking participants, write `engine:`; if you are adjusting the
behavior of a participant someone else might pick, write `engines:`.

## 2. `engine:` — naming the engines at play

A document's `engine:` key declares its **execution sequence**: the
engines that will run, in the order they will run. Because the key *names
participants*, writing it is committal — q2 treats an explicit list as the
author's complete statement of who plays, and three consequences follow:

1. **The implicit-fallback safety net turns off.** Normally, a
   computational language that no engine claims falls to whichever
   registered engine makes the strongest `Fallback` claim — jupyter by
   default, but any engine may declare fallback claims and outbid it (the
   "T4" tier in `engine-resolution.md` §4.3). An explicit `engine:` list
   disables that whole tier: if your list doesn't cover a language, no
   unlisted engine is quietly added to cover it. (A *listed* engine's
   fallback claims still catch leftovers — that is the explicit-fallback
   tier, T2.)
2. **Listed engines are present by declaration.** An engine's *interop*
   claims (knitr's reticulate taking `{python}`, for example) fire only
   when the engine is already present. Listing an engine makes it
   present, even in a document where it wins no language on its own.
3. **Order is execution order.** Engines run sequentially, each consuming
   the previous engine's output — so a generator engine must be listed
   before the engine that executes what it generates.

The key accepts three shapes:

```yaml
engine: knitr                          # scalar: a one-engine sequence
engine: [knitr, jupyter]               # array: N engines, in order
engine:
  - jupyter:                           # entry with config: the map's value
      kernel: python3                  # is threaded to the engine at
                                       # execute time
```

A per-entry config may also carry one reserved key, `claims:`, a
document-level **claim table** for the listed engine (see §3 — the table
semantics are identical on both keys). `claims:` is resolution metadata,
not engine configuration, so q2 strips it before the config reaches the
engine.

One q2-specific behavior deserves care: `engine:` is read from **merged
metadata**, so a project-level `engine:` in `_quarto.yml` works and
concatenates with a document's own list. When both layers name the same
engine, the *project's* entry wins (array layers concatenate project-first
and deduplication keeps the first occurrence); q2 warns about the conflict
and points at the `!prefer` merge tag for documents that need to override.
If what you actually wanted at the project level was engine
*configuration* — not forcing every document's sequence — the right key is
`engines:`, which is the subject of the next section.

## 3. `engines:` — configuring engines without naming any

The project-level `engines:` key configures the **engine registry**: the
pool of engines (built-ins plus discovered extensions) that resolution
draws from. Writing it never puts an engine into play. Its guarantees are
the mirror image of `engine:`'s consequences, and they are worth stating
as guarantees because they are what make the key safe to use project-wide:

- it never makes any document's sequence explicit (the implicit-fallback
  tier stays on);
- it never makes an engine "present" (no interop side effects);
- it never causes an engine to run that wouldn't have run anyway.

The one selection-adjacent influence it retains is **ordering** — and it
reaches a little further than a tie-break. The order engines are visited in
(the *candidate order*: `engines:` entries first, then extensions in the
order they were discovered, then the built-ins) does two jobs. It breaks
equal-strength claim ties — when two engines make the same-kind,
same-priority claim on a language, the earlier one wins it. And it sets the
order in which the chosen engines actually run: a document with `{r}` cells
knitr claims and `{julia}` cells a Julia extension claims runs them in
candidate order — the extension, then knitr — **not** in the order the
cells appear in the file. So `engines:` never *casts* anyone: claims decide
who runs, and the implicit-fallback net still fills the gaps. It only sets
the running order of whoever the claims chose. Configuration, not casting —
the configuration just happens to include sequence.

The key is a Q1-syntax-compatible array whose entries come in three forms:

```yaml
engines:
  - knitr                              # string: ordering only
  - path: ./my-engine.js               # Q1's external-engine loader;
                                       # reserved (see §4)
  - legacy-python:                     # name-keyed map: per-engine config;
      claims: [python]                 # `claims` is the only config key
```

The `claims:` value is a **claim table**: a complete replacement for the
engine's `_extension.yml` `claims:` block, with the same schema and the
same authority. "Complete" is the operative word — a table replaces the
engine's *entire* claim surface, so a language absent from the table is
simply not claimed (unless the table carries a universal `fallback:`
entry, which claims everything at the fallback floor, exactly as it would
in `_extension.yml`). Two consequences make tables the key's headline
feature:

- **A tabled engine is load-free.** Resolution answers its language
  claims from the table without loading any code — which is what lets a
  project's execution languages be resolved at index time, without ever
  loading a legacy, claims-less extension, from one block of YAML and no
  edit to the extension.
- **An empty table is a mask.** `claims: []` means "this engine claims
  nothing" — including, if you apply it to jupyter, disabling the
  universal fallback project-wide.

The full table semantics — source precedence, masking built-ins,
priority-based forcing, validation policy — live in
`engine-resolution.md` §3.3; this document only needs the shape.

Two policies round out the key. A map entry naming an engine that is not
in the registry is a **hard error at project load** — one failure, early,
before any document is rendered, with Q1's message. And although `engines:`
is a project-level key, q2 reads it from merged metadata, so a
document-frontmatter `engines:` block also takes effect — an implementation
artifact, not a supported surface; do not rely on it.

## 4. Differences from Q1

Readers coming from Quarto 1 should unlearn three things.

**Q1's `engine:` chose one engine; q2's chooses a sequence.** In Q1 the
key (or a top-level engine-name shorthand like `jupyter: python3`) named
the single engine that ran the whole document. q2 keeps every Q1 spelling
and generalizes the meaning: the array form declares N engines that run in
order, with per-language ownership divided among them by the resolution
tiers. A second, quieter difference hides in *where* the key is read: Q1
consulted only the file's own frontmatter, so an `engine:` in
`_quarto.yml` was silently inert; q2 reads merged metadata, so the project
layer participates (with the project-wins-on-duplicates rule from §2).

**Q1's `engines:` did loading and ordering; q2's does configuration and
ordering.** Q1's entries were engine names (ordering) and `{path: ...}`
objects that dynamically imported external engine modules. q2 replaces the
loading role entirely — engines arrive via `_extensions/` discovery, and
`path:` entries are reserved rather than honored — and adds a role Q1
never had: per-engine configuration via claim tables. The ordering role
carries over unchanged in spirit (user-listed engines are consulted
first).

**The selection/configuration boundary is sharper in q2.** Q1's `engines:`
order could effectively *select* the winner because Q1's claims were bare
scores and ties were common. q2's kind-tagged claims (`Primary` /
`Interop` / `Fallback`, kind dominating priority) make ordering rarely
*decide a contest* — a genuine equal-kind, equal-priority tie is uncommon.
What ordering still always does is set the **run order** of a multi-engine
sequence (§3): *who* runs is decided by `engine:` and by claims, but the
order they run in is candidate order, which `engines:` shapes. So `engines:`
stays on the configuration side of the line — it just configures sequence as
well as claims.

## 5. History of the two keys in q2 (condensed)

**`engine:`** has existed since the first engine-detection commit
(2026-01-07, `748856f50`) — already scalar, config-map, and shorthand
forms — and gained the array form with sequential multi-engine execution
(2026-05-29, `f34c20dbb`, #238). The epic reserves one key inside an
entry's config, `claims:`, a document-level claim table that replaces the
engine's claims (§2).

**`engines:`** had no q2 meaning before the TS Engines Epic — a project
that set it was setting an inert key. The epic gives it three roles: a
verbatim **wire pass-through** to TS engines (for Q1-API parity — engines
may read `project.config.engines` — narrowed to names), the **claim-table
entries** that are its first Rust-side semantics, and the **ordering
splice** in `build_engine_registry`.

## 6. Choosing between them: three worked one-liners

- *"This document should run knitr, then jupyter."* → `engine: [knitr,
  jupyter]` in the document. You are naming participants.
- *"Our legacy extension has no static claims and the language server
  can't index our project."* → in `_quarto.yml`:
  `engines: [{legacy-python: {claims: [python]}}]`. You are configuring a
  participant; every document resolves at index time and no document's
  sequence changes.
- *"knitr keeps grabbing our python cells via reticulate, project-wide."*
  → `engines: [{knitr: {claims: {r: primary}}}]`. A whole-table
  replacement that omits python — masking, again without touching any
  document's cast.
