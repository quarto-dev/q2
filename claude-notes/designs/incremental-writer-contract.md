# Incremental writer contract

The incremental writer (`pampa::writers::incremental`) edits a qmd
source file in place from a pair of ASTs: a baseline AST that
matches what was last produced from the source, and a new AST that
reflects the user's edits. It diffs the two structurally, copies
unchanged bytes from the original source, and re-serializes the
changed regions through the qmd writer.

This document describes the rules the writer obeys — what it
guarantees, what it forbids, and how callers must shape their inputs
to make the guarantees hold. It is the contract; implementation
specifics, file paths, and migration plans live in plans that
modify the writer.

**Companion doc:** [`provenance-contract.md`](provenance-contract.md)
covers the *producer* side — how transforms pick the right `SourceInfo`
shapes that this doc tells the writer how to consume. The two are
designed in pairs: if you change either contract, check the other.
The provenance doc also carries the `By::` constructor catalog with
atomicity flags that the §"Atomic-kind `Generated`" section below
draws on.

## The four primitives

The writer is one node in a four-primitive grammar:

| Primitive | What it does |
|---|---|
| **parse** | Lex/parse qmd source bytes into a parse-only AST. No transforms. |
| **transform** | Apply a pipeline's transform stages to an AST. Produces a same-shape AST at a different tier. |
| **reconcile** | Diff two ASTs structurally, producing a plan of `KeepBefore` / `UseAfter` / `RecurseIntoContainer` alignments. |
| **write** | Materialize the plan as qmd bytes — Verbatim-copy source bytes for `KeepBefore`, re-serialize through the qmd writer for `UseAfter` / `Rewrite`. |

The primitives are orthogonal. The writer is pipeline-agnostic: it
diffs the two ASTs it is given and writes accordingly, regardless
of what pipeline produced them. The caller picks which transforms
to apply (or none); the writer just diffs.

### Pipeline-tier discipline

The two ASTs handed to the writer must be at the **same pipeline
tier**. Same-tier means: both ASTs were produced by the same
sequence of transform stages, applied to inputs that were both
parsed from the same kind of source. The reconciler is
tier-agnostic — it diffs whatever it is given — but if the two
inputs do not share a tier, every Generated wrapper looks like a
new insertion and the output degrades to whole-document
re-serialization.

Two tiers are in use today:

- **parse-only**: the output of `parse_qmd_to_ast(content)`. Used
  by q2-debug, q2-slides, and the WASM demos.
- **q2-preview**: the output of
  `renderPageInProjectWithAttribution(path, …)`, i.e. post-q2-
  preview-pipeline AST. Used by ReactPreview's q2-preview path and
  the q2-preview SPA.

Future pipeline kinds are admitted without writer changes. The
caller composes parse and transform separately and hands the
writer two ASTs; the tier is implicit in whichever baseline the
caller passes.

## The byte-provenance contract

The writer materializes bytes constantly. Every Rewrite path emits
new bytes through the qmd writer; even Verbatim copies are a form
of materialization. The contract is not "no materialization" — that
phrasing is too blunt. It is more precise:

> The writer only emits bytes whose origin can be honestly traced
> to either **existing source bytes in the target file** (Verbatim
> copies, slot preimages via `preimage_in`) or **fresh AST the
> user constructed** (Rewrite paths fed by user-supplied AST
> nodes via the qmd writer's normal arms).

The case the contract forbids is the one where the writer would
emit bytes synthesized from a wrapper's slot children as flat
content in the parent file. The canonical example is include
expansion: an `IncludeExpansion` wrapper carries the included
file's blocks in a content slot. Emitting those blocks as flat
parent-file bytes would put bytes in `parent.qmd` whose provenance
is `foo.qmd` — dishonest at the parent-file boundary.

The writer's coarsen step prevents this case structurally rather
than catching it at write time. When the reconciler asks the
writer to recurse into a wrapper that is not editable inside (an
atomic CustomNode, an atomic-kind Generated, or any node with no
preimage in the target file), `coarsen` substitutes a safe
alignment — usually KeepBefore — before the qmd writer ever sees
the case. The qmd writer's arms for these wrappers thus become
`unreachable!()` in a well-formed pipeline: a debug-assertion
surface for coarsen bugs, not a user-facing failure mode.

This is why `incremental_write` returns `Result<(qmd, warnings),
Vec<DiagnosticMessage>>` — `Ok` is the normal path (write
succeeded; warnings carry any soft-drops); `Err` keeps its
pre-Plan-7 meaning, surfacing qmd-writer failures that bubble up
via `?` from the underlying serializer. Programmer errors —
invariant violations from coarsen bugs, structurally impossible
reconciliation states — do **not** flow through `Result`; they
`panic!()` / `unreachable!()` / `debug_assert!()` inline. This is
the idiomatic q2 pattern (see existing uses across
`pampa/src/writers/`) and the WASM-side surface is loud:
`console_error_panic_hook` is installed at module init, so a panic
becomes a JS exception with a full stack trace. Every user-facing
bad-edit case is handled by soft-drop, not by returning `Err`.

## The role-asymmetry contract on `Generated.from`

A `Generated` node's `from` field is a list of `Anchor`s, each
carrying a role and a `source_info` chain. Roles in use today:

- **`Invocation`**: the source token whose pipeline-time
  interpretation produced this node. E.g. the `{{< meta title >}}`
  shortcode bytes that resolved into the inlines now appearing in
  the rendered output.
- **`ValueSource`** (Plan 9): the metadata range whose value the
  node was synthesized from. E.g. the YAML byte range of
  `meta.title` that the title-block synthesizer read to build the
  rendered title block.
- **`Other("…")`**: extension-defined attribution. Carries
  whatever identity the extension wants; not interpreted by core.
- **`Dispatch`** (Plan 10, future): the Lua source location of a
  filter or shortcode handler that produced this node.

`preimage_in` — the writer's byte-range lookup — walks **only the
`Invocation` anchor**. All other roles, present and future, are
diagnostic-only. The writer never copies bytes from a non-
`Invocation` anchor's source range.

This asymmetry is load-bearing. A `ValueSource` anchor points at
YAML metadata bytes; copying those into a document body would
emit raw YAML in the middle of prose. A `Dispatch` anchor points
at Lua filter source; copying those bytes would emit Lua code as
prose. Both are correctness bugs. The writer prevents them by
never walking past the role discrimination.

Extension authors using `AnchorRole::Other("…")` can rely on this:
their attribution data will not be accidentally consulted by the
writer's byte-copy path, regardless of what they choose to point
it at. The role-asymmetry is the forward-compat guarantee.

## The unified editability predicate

`is_editable_inside(node, target_file_id) -> bool` decides whether
inner edits to a node are accepted. The same predicate is consulted
by two surfaces:

- React's read-only gate (Plan 2A's framework atomic gate)
  classifies regions in the rendered DOM and prevents the user
  from typing into uneditable regions in the first place.
- The writer's coarsen step uses it to decide whether to recurse
  into a container or soft-drop edits aimed at its interior.

Three structural reasons a node is not editable inside:

1. **Atomic CustomNodes** — types listed in `ATOMIC_CUSTOM_NODES`
   (`CrossrefResolvedRef`, `IncludeExpansion`). These represent
   single replaceable units. The user can replace them wholesale
   via a component menu; they cannot type inside them.

2. **Atomic-kind `Generated`** — `Generated` nodes whose `by.kind`
   is one of `"shortcode"`, `"filter"`, `"title-block"`,
   `"tree-sitter-postprocess"`. Pipeline-emitted content whose
   user-source is the invocation token (for shortcode) or whose
   source identity is the pipeline stage that produced it (for
   the others); not the resolved text the user sees.

3. **No preimage in target** — nodes whose `preimage_in(target)`
   returns `None`. This covers cross-file `Original` nodes
   (without a wrapper that pulls them into the target's
   provenance), synthesized containers like sectionize / footnotes
   / appendix that have empty anchor lists, and gappy `Concat`
   chains.

A node is editable inside iff it has byte-traceable preimage in
the target file AND is not an atomic CustomNode AND is not an
atomic-kind `Generated`.

The predicate is canonical on the Rust side; React consults an
equivalent TypeScript predicate that reads the same AST shape.
Keeping the two in lockstep is a discipline like the `ATOMIC_CUSTOM_NODES`
const / TS hand-mirror pairing.

## Soft-drop semantics

When a reconciliation alignment would target a non-editable region,
`coarsen` substitutes a safe alignment and emits a warning into a
warning sink. The write succeeds; the rejected edit is the only
casualty.

Five cases:

- **Inline-level UseAfter on a region where `is_editable_inside`
  returns false** (typically: user retyped resolved shortcode
  text). `coarsen` substitutes `KeepBefore` for the inline at the
  original-side index; the surrounding inline plan continues.
  Emits `Q-3-42`.

- **Block-level RecurseIntoContainer on a non-editable region**
  (user edited inside an include, or inside a synthesized-from-
  metadata container). `coarsen` substitutes `KeepBefore` for the
  wrapper. If the wrapper has preimage in target (atomic
  CustomNode whose `source_info` is `Original` covering the
  include token), the substitution lands in `Verbatim`. If it
  does not (no-preimage Generated container), the substitution
  lands in `Omit`; the container regenerates from baseline content
  on the next pipeline run. Emits `Q-3-43`.

- **Block-level UseAfter on an atomic CustomNode** — *let-user-
  win*. Kept as `Rewrite`; the qmd writer's CustomNode arm reads
  `plain_data` and emits the include syntax from a fresh user-
  edit-tagged CustomNode. No warning. This is the deliberate
  asymmetry: when the user explicitly destroys or replaces an
  atomic CustomNode through an explicit affordance (e.g. a
  component menu picker), the intent is unambiguous.

- **Block-level UseAfter on a no-preimage Generated container**
  — substitute `Omit`; the original container regenerates next
  run. There is no source position to anchor a `Rewrite` at, so
  let-user-win is not available. Emits `Q-3-43`.

- **KeepBefore on an atomic-kind `Generated` with empty `from`**
  — substitute `Omit`. The original content regenerates from
  baseline (the filter constructs it again, the title-block
  synthesizer reads the metadata again, etc.). No user edit was
  involved; this case is normal Coarsen flow, not a soft-drop in
  the user-facing sense. The only exception is the shortcode
  sub-case discussed below.

### Why soft-drop replaces hard-abort

The writer could have made every bad-edit case fatal: an
`AtomicViolation` variant returned as `Err`, causing the entire
save to fail until the user undoes the bad edit. Soft-drop is
better because:

- React (Plan 2A's read-only gate) is the primary safeguard. The
  writer is the contract guarantor. If React has a hole, the
  writer protects without losing the user's session.
- The user's *other* edits in the same save are not held hostage
  to the bad one. A user editing several paragraphs and
  accidentally typing into a shortcode resolution loses the
  shortcode edit, not the paragraph edits.
- The user-facing failure mode "the entire save was rejected" is
  not a recoverable state in an autosave context (hub-client and
  the SPA both persist on every keystroke; there is no discrete
  save the user can discard).

### User-facing diagnostic surface

Soft-drop emits warnings, not errors. Two codes:

- **`Q-3-42` — Shortcode edit dropped.** Inline-level cases:
  the user retyped over a shortcode-resolved, filter-decorated,
  or title-block-generated inline. The diagnostic body names the
  affected text and the source range of the invocation token.
- **`Q-3-43` — Generated content edit dropped.** Block-level
  cases: include-expansion recursion, synthesized-container
  recursion, synthesized-container replacement. The diagnostic
  body names the include `source_path` (for includes) or the
  metadata key (for metadata-derived containers), and an
  imperative instruction ("To edit this content, open `<path>`
  directly." / "This content is generated from metadata; edit
  `_quarto.yml` to change it.")

Both warnings carry source ranges and surface in Monaco as
squiggles. The autosave context makes both codes prone to
repeating on every keystroke; the diagnostic-ingest layer applies
suppress-after-3-by-source-range so the user is not flooded.

## Atomic CustomNodes

A CustomNode is *atomic* if it represents a single, indivisible
unit at the editing layer. The user cannot type inside one; they
can only replace it wholesale through an explicit affordance
(component menu, palette command).

The set of atomic CustomNode type names is declared in two places
that must stay in sync:

- **Rust**: `quarto_core::ATOMIC_CUSTOM_NODES: &[&str]` and the
  predicate `quarto_core::is_atomic_custom_node(type_name: &str)`.
- **TypeScript**: a hand-mirrored `ATOMIC_CUSTOM_NODES: ReadonlySet<string>`
  in `ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts`.

Built-in atomic types as of this writing: `CrossrefResolvedRef`,
`IncludeExpansion`. Extensions wanting to declare their own atomic
types will eventually do so via `_extension.yml` schema; until
that lands, the const set covers the cases.

### Atomic CustomNodes do not block let-user-win

The let-user-win Rewrite path for block-level UseAfter on an
atomic CustomNode is provenance-honest. When the user constructs
a fresh `IncludeExpansion` through React (with `plain_data =
{ source_path: "bar.qmd" }`) and the writer materializes
`{{< include bar.qmd >}}` into source, the bytes' origin is the
user's edit. The qmd writer's `IncludeExpansion` arm reads
`plain_data`, not `source_info`, and emits the include syntax —
the same arm whether the wrapper came from `IncludeExpansionStage`
(pipeline) or from React (user). That symmetry is what makes
let-user-win clean.

## Atomic-kind `Generated` and the shortcode-only invariant

Four `By::kind` values are classified as atomic by
`By::is_atomic_kind()`:

- `"shortcode"` — resolution of a `{{< … >}}` token
- `"filter"` — filter-emitted construction (e.g. `pandoc.Str(...)`)
- `"title-block"` — title-block synthesizer output
- `"tree-sitter-postprocess"` — tree-sitter postprocess synthesized
  whitespace and similar

These split into two structurally different cases at the writer's
`KeepBefore` branch:

| Kind | Source token in qmd? | Missing `Invocation` anchor means | Correct writer action |
|---|---|---|---|
| `shortcode` | Yes — `{{< … >}}` | Plan-6 stamper bug; the token bytes get lost in output | Debug-assert; `Omit` in release |
| `filter` | No — filter constructed the node | Expected (no source token exists) | `Omit` — regenerates next run |
| `title-block` | No — synthesized from metadata | Expected | `Omit` — regenerates next run |
| `tree-sitter-postprocess` | No — synthesized space etc. | Expected | `Omit` — regenerates next run |

Shortcode is the only kind that warrants a debug-assert on the
empty-`from` case. For the other three, empty `from` is the
normal shape — there is no source token to anchor at — and
regenerating from baseline is the correct behavior. For
shortcode, empty `from` means the stamper failed to attach the
token's source range, and `Omit` would silently lose the
`{{< … >}}` bytes the user wrote.

The asymmetry is intentional. Tests covering Coarsen's `Omit`
path must exercise all four kinds (filter / title-block /
tree-sitter-postprocess hit the regular Omit path; shortcode hits
the debug-asserted Omit path under `cfg(debug_assertions)`).

## Multi-inline dedupe

A single source token can resolve to multiple AST inlines. The
canonical case is a shortcode whose metadata value parses as
markdown:

    {{< meta title >}}

with `meta.title: "**Bold** Title"` resolves to three inlines:
`Strong[Str("Bold")]`, `Space`, `Str("Title")`. Each inline
carries the same `Generated { by: shortcode("meta"), from:
[Invocation -> Original{shortcode_token_range}] }` shape.

At the block level, both reconciliation inputs see the same
three-inline output, the surrounding `Para` is structurally
identical, and the alignment is `KeepBefore` over the whole Para
— one `Verbatim` of the whole Para's bytes. Correct.

At the inline level (when the user edits something else in the
same Para), the reconciler picks `RecurseIntoContainer` and walks
the inline plan. Without dedupe, each shortcode-derived inline's
`KeepBefore` would Verbatim-copy the shortcode token, emitting
the `{{< meta title >}}` bytes three times.

The dedupe rule: when iterating inline alignments, group
consecutive `KeepBefore` entries whose inlines' `Invocation`
anchors are `PartialEq`-equal, and emit `Verbatim` once for the
group using the anchor's preimage byte range.

`SourceInfo` derives `PartialEq`, and `Anchor` carries
`source_info: Arc<SourceInfo>`. `Arc<T>`'s `PartialEq` compares
the inner value, not the pointer, so structurally-equal anchors
in distinct `Arc`s still compare equal. This is what makes
dedupe work without identity-tracking machinery.

Dedupe consults `Invocation` only. Two inlines whose `Invocation`
anchors match but whose `ValueSource` or `Dispatch` anchors
differ still dedupe — the user is asking "which source token did
these come from", not "which metadata value" or "which Lua
file".

## Filter mutations versus constructions

Plan 4 distinguishes two kinds of filter activity:

- **Filter construction** — a filter emits a new node from
  scratch (`pandoc.Str("decoration")`). The result carries
  `Generated { by: filter, from: [] }`, classified atomic.
- **Filter mutation** — a filter modifies a node it received
  (`Str.text = upper(Str.text)`). The result keeps the
  `Original` source_info of the input node, *not* `Generated`.
  Not classified atomic.

A user edit through React on a filter-mutated `Str` produces an
unusual round-trip. The user types "world" over the filter-output
"HELLO"; the writer Rewrites "world" to source bytes; the next
pipeline run filters "world" → "WORLD". For idempotent filters
(like uppercase) this is fine — the typed text round-trips through
the filter to itself. For non-idempotent filters
(`x => upper(x) + "!"`) the typed text gets a `!` appended on
every save, which is confusing.

This corner is accepted, not fixed, because:

- Revising Plan 4 to track filter mutations distinctly from
  plain `Original` would be a notable type-system change.
- Plan 7a's runtime user-filter idempotence detection catches the
  AST-level non-idempotence that would actually corrupt
  round-trip.
- Plan 3's idempotence test enforces the contract for built-in
  filters at CI time.

Users who write non-idempotent filters get a runtime warning
(Q-3-44 / Q-3-45) and can decide whether the trade-off is
acceptable.

## Design rationale & evolution

This section captures the *why* behind decisions that read as
arbitrary out of context.

**Soft-drop replaces hard-abort.** Earlier sketches of the writer
modeled bad-edit cases as a fatal `AtomicViolation` returned as
`Err`. That variant was never implemented — soft-drop subsumes it
before reaching code. The reason is the autosave context: there
is no discrete "save" affordance the user could use to discard a
bad edit, so a save-rejecting error trades one keystroke loss
for an entire session's worth of edits held hostage. Soft-drop
keeps the surface area of failure minimal — only the bad edit is
lost — and gives the contract guarantor a way to protect honesty
without punishing the user.

**The writer is pipeline-agnostic by signature.** The WASM entry
takes a baseline AST as an argument rather than parsing the
original qmd internally to synthesize one. This makes the writer
ignorant of which pipeline produced its inputs; future pipelines
land without writer changes. The caller composes parse and
transform; the writer just diffs. The change also removes the
writer's dependency on `RenderContext`, `SystemRuntime`,
`Format`, and pipeline construction machinery — its surface
becomes three strings in and one JSON envelope out.

**No `pipeline_kind` parameter.** The pipeline tier is implicit
in the baseline AST the caller passes. A `pipeline_kind`
parameter would be a redundant claim that the caller could get
wrong; making it implicit removes one consistency requirement
the writer would have to enforce.

**`Invocation`-only walking is a forward-compat surface for
extensions.** An extension author who attaches attribution via
`AnchorRole::Other("their-thing")` can rely on the writer not
walking their data. They get a free guarantee: whatever they
point at, the writer will not turn it into rendered bytes by
accident. This makes the role discrimination both a correctness
mechanism (for `ValueSource`, `Dispatch`) and an extensibility
mechanism (for `Other`).
