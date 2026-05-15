# L11 — Listings epic close-out

**Date:** 2026-05-08
**Beads:** `bd-qb4o` (parent `bd-61cd`).
**Parent plan:** `claude-notes/plans/2026-05-05-listings-epic.md`
**Predecessor phases:** L0–L9 (closed); L10 (open, deferred).
**Status:** Draft.

## Goal of this phase

Compile the per-phase follow-up `bd` log into a single epic
report, confirm full-workspace verification on a clean tree,
and survey the outstanding follow-ups for any that are
worth pulling in *now* before declaring the epic delivered. L10
(migration docs + LLM skill) is intentionally left open — the
user wants to defer that until the Q2 docs site exists, so it
gets folded into the docs site's migration-guide work later.

## What this close-out is and is not

- **Is**: a roll-up of follow-up issues that were filed during
  L0–L9, an analysis of which (if any) are quick wins worth
  taking before closing the epic, and a list of the residual
  verification steps that still need to run.
- **Is not**: a re-implementation of the listings feature. All
  user-visible scope (built-ins, custom templates, categories
  sidebar, dep-graph integration, post-render upgrade, RSS
  feeds) shipped through L0–L9 and is in `main` (epic plan
  `§"Phase"` table records the merge hashes).
- **Is not**: the migration documentation. That's L10
  (`bd-hzsi`, deferred until the Q2 docs site exists).

## Verification checklist

These are the L11 plan's verification line items, separately
from the follow-up table below.

- [ ] `cargo xtask verify` runs clean on a fresh checkout
  (full leg — Rust + hub-client build + tests).
- [ ] `claude-notes/designs/document-profile-contract.md`
  change log already covers every profile-version bump in
  this epic (v3 → v4 from L0, v4 → v5 from L6). **No new
  entry required for L7–L9** — they consumed
  `listing_content_globs` and `listing_item` rather than
  adding new profile fields. Confirmed against the file at
  L11 drafting time.
- [ ] Hub-client renders listings end-to-end via WASM (real
  browser session, per CLAUDE.md §"End-to-end verification").
  Smoke: open a multi-page project containing a listing host
  in hub-client, edit a content page, see the listing host's
  preview update with L1 fallbacks. **Already filed as
  separate issues** — `bd-ra5j` (categories sidebar smoke)
  and `bd-khuj` (template-diagnostics smoke) — so this can
  be deferred to those rather than blocking close-out, *or*
  done here. Decision in §"Decisions to confirm" below.

## Outstanding follow-ups (collated)

All open issues filed against listings phases as
`discovered-from`, plus two issues that originated in L1
session work but were not linked at filing time (`bd-a3we`,
`bd-zzke`).

Key:
- **Source phase** is the issue this was discovered-from.
- **bd-x5r4**, **bd-khuj**, **bd-but3** are open against the
  same time window but are *not* listings issues (they trace
  back to `bd-xdnk` doctemplate diagnostics or `bd-w5ov`
  math-mode rendering); not included.

| ID         | Pri | Type    | Source | Title                                                                                | Quick-win? |
|------------|-----|---------|--------|--------------------------------------------------------------------------------------|------------|
| `bd-tmka`  | p2  | feature | L8     | WASM/VFS-aware custom listing template loading                                       | No         |
| `bd-nwyp`  | p2  | bug     | L5     | Audit listing config parsing for PandocInlines / yaml-markdown-syntax-error fallthrough | No      |
| `bd-xs2u`  | p2  | bug     | L3     | Em-dash / en-dash in document titles breaks something in hub-client                  | No         |
| `bd-57y4`  | p2  | task    | L3     | Vendor and integrate `quarto-listing.scss` with theme-CSS pipeline                   | No         |
| `bd-0jyl`  | p2  | task    | L3     | Source-info threading through listing markdown re-parse                              | No         |
| `bd-0fd0`  | p2  | task    | L3     | Lua filter injection slot between generate and render transforms                     | No         |
| `bd-a3we`  | p2  | feature | L1*    | WASM VFS: populate `PathMetadata.modified` from Automerge change history             | No         |
| `bd-xhvs`  | p3  | bug     | L9     | Escape `]]>` in CDATA-wrapped feed bodies                                            | **Yes**    |
| `bd-3sa5`  | p3  | feature | L9     | HTML-aware truncation in `extract_first_para_html`                                   | No         |
| `bd-d8go`  | p3  | feature | L9     | `date_format` doctemplate pipe                                                       | No         |
| `bd-mae2`  | p3  | feature | L9     | Custom feed templates (`feed.template:`)                                             | No         |
| `bd-4ho9`  | p3  | task    | L9     | Validate against W3C feed validator                                                  | Maybe      |
| `bd-ir8n`  | p3  | feature | L9     | Inline-code-style syntax-highlight class maps in full feeds                          | No         |
| `bd-yd4q`  | p3  | feature | L9     | Math handling in full feeds                                                          | No         |
| `bd-u4ow`  | p3  | task    | L8     | docs/ page for custom listing templates                                              | No (defer with L10) |
| `bd-ubjo`  | p3  | feature | L8     | Broader path resolution for YAML-declared paths                                      | No         |
| `bd-bpdz`  | p3  | task    | L7     | Reader extension surface for L9 (RSS feed extraction)                                | Done in L9 |
| `bd-rvpd`  | p3  | task    | L7     | Source-span threading for L7's Q-12-13 (and L9 diagnostics)                          | No         |
| `bd-bqf2`  | p3  | task    | L6     | Shared shape walker for `parse_listings` + `extract_content_globs`                   | No         |
| `bd-ra5j`  | p3  | task    | L5     | Hub-client browser smoke for categories sidebar                                      | Maybe (L11 verification) |
| `bd-754f`  | p3  | task    | L5     | Review category click-handler encoding scheme (b64+percent-encoding)                 | No         |
| `bd-99ru`  | p3  | task    | L5     | Localize listing category sidebar labels (Categories, All)                           | No         |
| `bd-0wyo`  | p3  | task    | L3     | Server-precomputed `other_metadata_html` for default listing                         | No         |
| `bd-8h9o`  | p3  | task    | L1     | Filter unresolved shortcodes in image-src auto-fill                                  | No         |
| `bd-zzke`  | p3  | chore   | L1*    | Consolidate six divergent `inlines_to_(plain_)text` helpers                          | No         |
| `bd-varx`  | p4  | task    | L9     | Hoist `append_to_rendered_header` + `escape_html_attr` to a shared util              | **Yes**    |
| `bd-sh4h`  | p4  | feature | L9     | Atom 1.0 emission                                                                    | No         |
| `bd-i4wv`  | p4  | task    | L9     | Real version string in feed generator (replace `quarto-2`)                           | No (blocked) |
| `bd-2vl0`  | p4  | task    | L9     | Q-12-15 dedup (per-project, not per-host)                                            | **Yes**    |
| `bd-udlt`  | p4  | feature | L9     | Title placeholder substitution from rendered HTML                                    | No         |
| `bd-eips`  | p4  | feature | L9     | `format.metadata.description` as channel description fallback                        | No         |
| `bd-fvuy`  | p4  | chore   | L8     | Q-12-10 catalog title/message inconsistency                                          | **Yes**    |
| `bd-fx23`  | p4  | task    | L7     | Defensive percent-encoding of `listing.id` in L7 image marker                        | Maybe      |
| `bd-399t`  | p4  | task    | L7     | Docs callout: L7 listings preview is CLI-only (defer with L10)                       | No (defer with L10) |

\* = filed during the phase's session but not via `discovered-from`
to a phase issue (origin is recorded in commit history /
plan files instead).

**Total:** 33 listings follow-ups open. Of those, **3** are
clean quick wins (`bd-fvuy`, `bd-varx`, `bd-2vl0`,
`bd-xhvs`); a few more are plausible if scope is small.

## Quick-win analysis

The four candidates I flagged as **Yes** in the table are all
mechanical, single-file (or near-single-file) changes that don't
require new design decisions. Brief rationale + estimated scope
per:

### `bd-fvuy` — Q-12-10 catalog title/message broaden (chore, p4)

**Site:** `crates/quarto-error-reporting/error_catalog.json`,
`Q-12-10` entry (line ~799–805).

The current catalog title is *Listing Markdown Re-parse
Diagnostics*, but the same code is emitted for two distinct
classes of failure:
1. doctemplate compile/render errors on a custom template
   (L8 path), and
2. re-parse diagnostics from feeding the rendered markdown back
   into pampa (L3 path).

Two mechanical options:
- **Broaden the existing entry** to *Listing Template Compile
  or Re-parse Diagnostic* (one-line edit, no code changes,
  message_template kept generic).
- **Split into Q-12-10 (re-parse) + a new Q-12-N (compile)**
  and update the L3/L8 emit sites to route accordingly. About
  4 file edits.

**Recommendation:** broaden, not split — splitting adds
noise without changing user behavior, and Q2's diagnostic
catalog policy isn't strict enough to require per-class codes
yet. ~10-line change, no test churn.

### `bd-varx` — Hoist two helpers to a shared util (task, p4)

**Sites:** `crates/quarto-core/src/transforms/website_favicon.rs:91-180`
(currently private `append_to_rendered_header` and
`escape_html_attr`) and
`crates/quarto-core/src/project/listing/feed/link_inject.rs:108-160`
(verbatim duplicates with self-documenting comments pointing at
the duplication).

Both helpers are <30 lines combined. The natural home is a new
small module `crates/quarto-core/src/transforms/header_inject.rs`
or extension of `website_config.rs`. Both call sites already
have unit tests for `escape_html_attr`; consolidating them is
roughly:
1. Move the two `fn`s to the new module, make them `pub(crate)`.
2. Update both call sites to use the shared form.
3. Delete the duplicate test in `link_inject.rs` (or move it).

~50 lines diff, no behavior change, refactor-only.

### `bd-2vl0` — Q-12-15 dedup per-project (task, p4)

**Site:** `crates/quarto-core/src/project/listing/feed/stage.rs:586-855`.

Today `ListingFeedStageTransform` emits one `Q-12-15` per
host page that has `feed:` configured but no
`website.site-url`. For an N-host project, the user gets N
copies of the same warning.

Two shapes:
- **`ProjectIndex` flag** (clean): set
  `already_warned_no_site_url: AtomicBool` on
  project-shared state, claim it via
  `compare_exchange`. Requires a stable handle to project-
  shared state in this transform.
- **`ctx.diagnostics` post-hoc dedup** (uglier but
  self-contained): rely on a near-end pass that elides
  duplicate `Q-12-15` entries before user emission.

The second is a localized one-file change (~30 lines + a
test); the first requires a tiny addition to project-shared
state plumbing.

**Status caveat:** the issue is p4 because in practice the
N-host case is unusual (most users have one feed-host
website). A "quick win" framing is fair only if we want the
code-cleanliness for future projects.

### `bd-xhvs` — Escape `]]>` in CDATA-wrapped feed bodies (bug, p3)

**Site:** `crates/quarto-core/src/project/listing/feed/complete.rs:245-255`.

Today the substitution layer wraps the rendered first-paragraph
HTML in `<![CDATA[…]]>`. If the body literally contains `]]>`
the CDATA terminates early and the XML is malformed.

The standard XML fix is to split the body at every `]]>` and
emit two CDATA sections joined at the boundary:
`]]><![CDATA[`. ~5-line change, ~1 unit test (a body
containing `]]>` produces parseable XML).

**Recommendation:** this is the strongest quick-win
candidate — it's a defensive correctness fix in a
well-tested file with snapshot tests already in place. The
trigger for a real complaint is rare ("typical Quarto-
rendered output never contains `]]>`") but the fix is
two-digit lines, so the risk/reward is one-sided.

## Quick-win recommendation

If the user wants to land *something* now, the natural minimum
is just `bd-xhvs` and `bd-fvuy` together: one defensive bug fix
+ one diagnostic catalog edit, ≈30 lines combined, both
isolated, both shippable in one commit.

`bd-varx` (hoist) is a nice tidy-up but shows up as a refactor
on the diff and isn't load-bearing for the close-out — easy to
defer to whoever next touches `link_inject.rs`.

`bd-2vl0` (dedup) is correct in principle but the user-facing
impact of N copies of the same warning is small; not blocking.

A more ambitious "while we're here" bundle would also pull in
`bd-bpdz` (already done — close in close-out paperwork) plus
maybe `bd-bqf2` (shared shape walker) — but `bd-bqf2` is
larger than its priority suggests and would re-touch the
dep-graph code we just stabilized in L6.

**Default recommendation: skip the quick-win bundle.** Land
L11 as a paperwork-only close-out (this plan doc, the
verification check, the contract-doc sanity check, and a
hub-client smoke if we want to roll `bd-ra5j`/`bd-khuj` into
this phase). The follow-ups are all already filed; future
sessions can pick them up by priority. Including:

- We just landed L9, which itself is the largest single
  listings PR in the epic. Adding more code on top before
  declaring the epic delivered increases the chance of
  introducing a regression.
- The follow-ups span four different code areas
  (`feed/complete.rs`, `error_catalog.json`,
  `link_inject.rs` + `website_favicon.rs`, `feed/stage.rs`);
  bundling them into one commit muddies the bisect surface.
- The two highest-priority items (p2) are not quick-win
  shaped — they need real design work or cross-crate
  coordination.

Open question for the user: which of these three options:
1. **Paperwork-only L11** (this doc + verify + contract-doc
   re-check + close `bd-qb4o`). Defer all follow-ups.
2. **Paperwork + the strongest quick win** (`bd-xhvs` +
   `bd-fvuy`).
3. **Paperwork + quick wins + close `bd-bpdz`** (which is
   already complete, just needs a paperwork close).

## Decisions to confirm with user

1. **Hub-client smoke.** Run a real browser session as part of
   L11, or close `bd-qb4o` and rely on the already-filed
   `bd-ra5j` (categories smoke) + `bd-khuj` (template
   diagnostics smoke) to track that work?
2. **Quick-win scope.** Per §"Quick-win recommendation".
3. **L10 disposition.** Leave `bd-hzsi` open (status quo);
   confirm that the docs-site epic is the right home for it.
4. **`bd-bpdz` paperwork.** It was filed as
   "Reader extension surface for L9 (RSS feed extraction)"
   from L7's session, with the design intent that L9 would
   build on it. L9 implemented that design; the issue should
   be closeable as "delivered in L9 (`bd-o90m`)". Confirm
   close.

## Work items (this phase)

Once the above decisions are made, the actual L11 work is
small:

- [ ] Create this plan file (this commit).
- [ ] If decision (4) = close: `br close bd-bpdz --reason
      "Delivered in L9 (bd-o90m); reader extension shipped
      in feed/reader_ext.rs"`.
- [ ] If decision (2) ≠ paperwork-only: implement the
      chosen quick wins, each as its own commit, with full
      `cargo xtask verify` between commits. (Per CLAUDE.md
      §"GIT PUSH POLICY" and §"End-to-end verification".)
- [ ] Run `cargo xtask verify --skip-hub-build` (or full
      `cargo xtask verify`) and confirm clean.
- [ ] If decision (1) = run smoke: bring up hub-client
      against a multi-page listings fixture, edit a content
      page, confirm host preview updates with L1 fallbacks
      (no L7 in hub-client per L7 bracketing rule 3).
      Record the smoke in this plan file. Also close
      `bd-ra5j` and `bd-khuj` if their independent scopes
      are also covered.
- [ ] If decision (1) = defer: leave `bd-ra5j` and
      `bd-khuj` open as the smoke trackers.
- [ ] Update epic plan §"Phase" table: mark L11 closed with
      merge hash; mark L10 still open (status quo).
- [ ] `br close bd-qb4o --reason "L11 delivered: <summary>"`.
- [ ] `br sync --flush-only && git add .beads/ && git
      commit`.

## Acceptance criteria

L11 is done when:
- `cargo xtask verify` is green on the close-out branch.
- `claude-notes/designs/document-profile-contract.md` is
  reconfirmed as up-to-date for the listings epic (no
  pending entries).
- This plan file records the resolution of every decision in
  §"Decisions to confirm with user".
- `bd-qb4o` is closed.
- The listings epic plan §"Phase" table shows L11 closed.
- Either: a hub-client smoke is recorded here *or* `bd-ra5j`
  + `bd-khuj` remain open as explicit smoke-tracker tickets.

## What's next after L11

- **L10** (`bd-hzsi`) — Q1 → Q2 migration docs + LLM skill —
  remains open. Will be picked up when the Q2 docs site
  exists and migration content has a publication target.
- **Listings follow-ups** — 33 open issues per the table.
  Highest-priority unblocked items:
  - `bd-tmka` (WASM/VFS custom-template loading) — needed
    before hub-client users can author custom templates.
  - `bd-57y4` (theme-CSS integration for
    `quarto-listing.scss`) — needed before listings styling
    matches Q1 in the hub-client preview.
  - `bd-0fd0` (Lua filter slot between generate and render)
    — architectural; will probably come out of the broader
    extension-points epic when it's filed.

These three are the natural next-quarter listings work; the
remainder are p3/p4 polish.
