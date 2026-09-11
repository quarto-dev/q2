# Frontmatter reader splits on every `---`, truncating YAML values that contain one (bd-mjo6ao32, GH #671)

**Date:** 2026-09-10
**Braid:** bd-mjo6ao32 (related: bd-xs2u)
**GitHub issue:** https://github.com/quarto-dev/q2/issues/671 (filed by rundel)
**Branch:** `braid/bd-mjo6ao32-frontmatter-reader-splits-every` off `main` at `5a12a773`.
**Status:** Option A approved 2026-09-10; implemented, full `cargo xtask verify` green, committed on the topic branch (`79d352ba`, amended with this plan update). Phase 3 (close strand, GH comment, push) awaits the user.

## Triage verdict

**Ready to implement, reader-side only.** The tree-sitter parser is not at
fault. The defect is a five-line helper in pampa's metadata reader,
`extract_between_delimiters` in `crates/pampa/src/pandoc/meta.rs`, which
splits the raw metadata text on *every* `---` instead of on delimiter
*lines*. No qmd-writer change is needed (details below), which answers the
question raised when this was handed over.

## Issue summary

Since PR #290 the qmd writer canonicalizes every em dash in a markdown-parsed
frontmatter string to `---`. On re-read, the metadata is cut at that point:

| Value shape | Writer emits | Re-read result |
| --- | --- | --- |
| single-line `"Hello — world"` | `description: Hello --- world` (plain scalar) | YAML becomes `description: Hello`; **every later key silently dropped**, no diagnostic |
| multi-line literal block with `—` | `description: "Hello ---\nworld."` (double-quoted) | YAML becomes `description: "Hello` → unterminated quote → **Q-0-99**, metadata empty |
| hand-typed `"a --- b"` | n/a (reader only) | same Q-0-99 |

Pandoc 3.9 reads all three inputs correctly (verified locally); Pandoc ends a
YAML block only at a line consisting of `---` or `...`.

In-the-wild hit: quarto-web `docs/blog/_archive/posts/2026-04-14-chrome-headless-shell/index.qmd`
(multi-line form, so it errors rather than silently truncating).

## Root cause

### The parser is correct

The scanner (`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`,
`parse_minus`, the `MINUS_METADATA` branch) opens metadata only on a `---`
line at document/block start, then loops line by line: after each newline it
counts `-` characters **starting at column 0**, and closes only when it sees
exactly three, followed by optional spaces/tabs and a newline. It never
looks at `---` mid-line, and an indented `  ---` (e.g. inside a YAML block
scalar) does not close either. The `-v` concrete syntax tree for the
hand-typed case confirms the node spans the whole block:

```
document: {Node document (0, 0) - (6, 0)}
  metadata: {Node metadata (0, 0) - (4, 0)}    ← through the closing --- line
  section: {Node section (4, 0) - (6, 0)}
```

So the `RawBlock { format: "quarto_minus_metadata", text }` handed to the
metadata reader contains the full, correct YAML block. (Construction sites:
`treesitter_utils/document.rs`, `section.rs`, `fenced_div_block.rs`, and
`treesitter.rs` for list items — all pass the node text through unchanged.)

### The reader is not

```rust
// crates/pampa/src/pandoc/meta.rs:567
fn extract_between_delimiters(input: &str) -> Option<&str> {
    let parts: Vec<&str> = input.split("---").collect();
    if parts.len() >= 3 { Some(parts[1].trim()) } else { None }
}
```

`parts[1]` is everything between the *first* and *second* `---` anywhere in
the text. A `---` inside a value therefore ends the YAML there. What happens
next depends only on whether the cut lands inside a quoted scalar (YAML
error → Q-0-99) or a plain one (valid but truncated YAML → silent key loss).

The caller has a second, smaller latent bug on the same lines:

```rust
let content = extract_between_delimiters(&block.text).unwrap();
let yaml_start = block.text.find(content).unwrap();
```

`find(content)` searches from the start of the text, so it can match inside
the opening delimiter when the trimmed content begins with `-` and is short
enough (a YAML block whose first line is `-`, i.e. a top-level list, finds
offset 0 instead of 4). This mis-anchors every source span in the metadata.
Fixing the extractor to return byte offsets removes the `find` entirely.

This helper predates the crate rename (`git log -S 'split("---")'` reaches
back to `d86b8ecf`); it was never load-bearing until the writer started
emitting `---` inside values.

### Why the writer needs no change

- The writer's YAML is valid in every shape probed. `yaml_rust2`'s emitter
  already quotes scalars that *begin* with `---` (`title: "---"`,
  `sub: "--- foo"`) and emits mid-scalar dashes plain (`tail: foo ---`,
  `list: [x --- y]`, nested maps likewise). A `---` inside a plain scalar
  is legal YAML; only column-0 `---` is a document marker.
- Multi-line values are emitted double-quoted with `\n` escapes, so a
  line consisting of `—` alone can never become a column-0 `---` line.
- Quoting on the writer side would not fix anything: the hand-typed
  `"a --- b"` case is already quoted and still fails, because the split
  happens *before* the YAML parser ever sees the text.
- Re-read, `Hello --- world` goes through the metadata markdown pass and
  `apply_smart_typography` turns it back into `—` (verified for the en-dash
  sibling: `Hello -- world` reads as `Str "–"`). So once the reader is
  fixed, the PR #290 round-trip closes correctly with no writer work.

Alternative considered and rejected: make the writer keep the literal `—`
(or `\—`) inside frontmatter. It would mask the symptom for the writer's
own output but leave hand-typed `---` broken and would contradict PR #290's
deliberate canonicalization.

## Why the reader re-scans text it already parsed

`minus_metadata` is an **external token** (`grammar.js` externals list, line
~1121), inherited from upstream tree-sitter-markdown, where `minus_metadata` /
`plus_metadata` are opaque leaves because a Markdown grammar has no business
parsing YAML. External tokens cannot have children, so the tree hands pampa a
single `metadata` node covering `---` … `---` with no inner structure. The
scanner knows exactly where the body starts and ends while it runs, but it
throws that away when it emits one token. `treesitter.rs:701` therefore
takes `node.utf8_text()` of the whole node, wraps it in a
`RawBlock { format: "quarto_minus_metadata" }`, and the reader has to find
the body again. `split("---")` was the quickest way to do that and was never
load-bearing until PR #290 made `---` appear inside values.

Two facts constrain the fix:

- The delimited RawBlock is a **transient carrier**: it is consumed by the
  filter in `readers/qmd.rs:199` immediately after tree conversion and never
  reaches JSON, native, snapshots, quarto-core, or the TS packages (grep
  confirms only pampa and its tests know the format string). Changing its
  shape is a pampa-internal decision.
- The scanner already uses the "phase is told by `valid_symbols`" idiom for
  `_fenced_div_start` / `_fenced_div_end` (`scanner.c:649-654`), so a
  multi-token metadata block needs no new persistent scanner state.

## Options

### Option A — the grammar exposes the body range (recommended)

Replace the single external token with three: `_minus_metadata_start`,
`minus_metadata_body` (aliased to something like `yaml`), `_minus_metadata_end`.

```js
metadata: $ => seq($._minus_metadata_start, alias($._minus_metadata_body, $.yaml), $._minus_metadata_end)
```

Scanner:
- `start`: the existing entry conditions (column 0, exactly `---`, newline,
  next line not blank). It still needs to *confirm* a closing line exists
  before committing (otherwise `---` is a thematic break), so it does the
  same look-ahead loop as today but calls `mark_end` after the opening line
  and lets the look-ahead run past it. `mark_end` exists precisely for this.
- `body`: valid only after `start`; consume lines until the first column-0
  `---` (+ optional whitespace) line, `mark_end` **before** it, emit.
- `end`: consume that line, emit.

pampa: the `"metadata"` arm in `treesitter.rs:701` (the only site) reads
the `yaml` child's text and range; the RawBlock carries the YAML only, with
`source_info` = the child's range. `extract_between_delimiters` and the
`block.text.find(content)` hazard are deleted. `rawblock_to_config_value`
parses `block.text` directly. Hand-built RawBlocks in
`test_meta.rs`, `test_rawblock_to_config_value.rs`, `test_yaml_tag_regression.rs`
drop their `---` lines.

Blast radius: 5 corpus files / 9 `(metadata)` expectations become
`(metadata (yaml))`; `tree-sitter generate; tree-sitter build; tree-sitter test`;
one dispatch arm in pampa; three test files' fixtures. Error recovery for an
unterminated block is unchanged in outcome (no `start` is emitted unless a
close exists, exactly as today).

Why recommended: the body range comes from the parse, so there is nothing
in Rust to keep in sync with `scanner.c`. This is the concern raised in
review and it is the only option that removes the duplication rather than
shrinking it.

### Option B — strip the token's first and last lines

Keep the single token. In the `"metadata"` arm, drop the first line and the
last line of the node text, no `---` matching at all, and build the RawBlock
from the remainder with an offset-adjusted `source_info`. Assert (hard error,
not `debug_assert`) that the dropped lines are `---` + optional whitespace, so
any future scanner drift fails loudly instead of truncating silently.

Coupling is to the token's *definition* (one delimiter line on each end), not
to the scanner's dash-counting. Smallest change; no grammar rebuild. Still a
convention rather than a structural guarantee.

### Option C — line-based re-scan in Rust (rejected)

The original proposal: find the first column-0 `---` line after the opening
one. Works, but it re-implements the scanner's closing rule in Rust with no
mechanism keeping the two in agreement. Rejected in review.

## Proposed fix

**Option A**, unless the user prefers B as a hotfix first. Either way the
qmd writer is untouched. The Phase 0 tests below are written so that they
pass under both A and B (they test reader behaviour, not the mechanism),
except the corpus expectations, which are A-only.

Deliberately **not** in scope: accepting `...` as a closing delimiter. The
scanner does not recognize it; Pandoc does. Filing that as a separate parity
strand is proposed in follow-ups. (Under Option A that later becomes a
scanner-only change with no Rust counterpart, which is another point in A's
favour.)

## Work items

Phase 0 — tests first (must fail before the fix):

- [x] `tests/integration/test_rawblock_to_config_value.rs` (or a new
  `test_frontmatter_delimiters.rs`, registered alphabetically in `main.rs`):
  full parse of the three issue inputs produces all keys and no
  diagnostics; `author` is present after a `description` containing `---`;
  an indented `  ---` line inside a literal block scalar survives; closing
  line with trailing spaces; CRLF document.
- [x] Source-tracking check (`test_metadata_source_tracking.rs` style): the
  `author` key's span points at the right bytes when an earlier value
  contains `---`; a top-level-list frontmatter (`---\n- a\n---`) anchors the
  YAML at offset 4, not 0 (the `find` hazard).
- [x] Round trip: parse → qmd writer → parse for the single-line and
  multi-line em-dash inputs; second metadata equals the first
  (`Str "—"` restored).
- [x] Option A only: corpus test in `test/corpus/` with `---` inside a
  quoted value and inside a plain value, expecting `(metadata (yaml))` with
  the body range excluding both delimiter lines; run `tree-sitter test`,
  confirm failure.
- [x] Run the new tests, confirm each fails for the expected reason.

Phase 1 — implementation (Option A):

- [x] Scanner: split `MINUS_METADATA` into start/body/end tokens per the
  sketch; keep the blank-line-after-opening → thematic-break behaviour.
- [x] `grammar.js`: new externals, `metadata` rule; `tree-sitter generate;
  tree-sitter build; tree-sitter test`; update only the `(metadata)`
  expectations touched by this change.
- [x] pampa `treesitter.rs` `"metadata"` arm: read the `yaml` child; RawBlock
  text = body, `source_info` = body range.
- [x] `meta.rs`: delete `extract_between_delimiters` and the `find`;
  `rawblock_to_config_value` parses `block.text` directly. Update the
  hand-built RawBlocks in the three test files.
- [x] `cargo nextest run --workspace`.

Phase 1 — implementation (Option B, if chosen instead):

- [ ] `treesitter.rs` `"metadata"` arm strips first/last line with a hard
  shape assertion; RawBlock carries body only; `meta.rs` as above.

Phase 2 — end-to-end (record invocation + output here):

- [x] The issue's three `printf … | cargo run --bin pampa` pipelines; the
  re-read `jq -c .meta` must show both keys and `Str "—"`.
- [x] `cargo run --bin q2 -- render` of a fixture with an em dash in
  `description:` and a later `author:`; inspect the HTML for both.
- [x] Full `cargo xtask verify` (pampa and tree-sitter-qmd are in the WASM
  leg via `wasm-quarto-hub-client`).

Phase 3 — wrap-up:

- [ ] Close bd-mjo6ao32; comment on GH #671 with the fix (needs user
  approval before posting).
- [ ] Decide on follow-ups below.
## Follow-ups (not part of this fix; need a decision)

- **`...` closing delimiter parity.** Pandoc closes a YAML block on `...`
  too; our scanner only recognizes `---`. Propose a `parity`-labelled strand.
- **bd-xs2u** ("Em-dash / en-dash in document titles breaks something in
  hub-client", 2026-05-06, never reproduced). It predates PR #290, so the
  writer was not emitting `---` then, but any hub-client path that
  re-serializes frontmatter through the qmd writer today would hit exactly
  this bug. Linked as `related`; worth re-testing after the fix lands.
- **bd-wl58atds** (filed during Phase 2, `discovered-from` this strand):
  a multi-line literal block scalar containing a multi-byte character,
  followed by a plain scalar, makes quarto-yaml's content-provenance
  derivation return `None` and pampa warn "YAML string scalar has no
  content provenance". Verified pre-existing on `main` with a side-by-side
  build; the quarto-web file from the issue emits it 6 times both before
  and after this fix.
- Observed, no action proposed: a metadata string `"- foo"` is parsed as
  markdown and comes back as a bullet list, so the writer emits `* foo`.
  That is the markdown-metadata contract, not a delimiter problem.

## Reproduction record (2026-09-10, `main` @ 5a12a773)

```
$ printf -- '---\ndescription: "Hello — world"\nauthor: Z\n---\n\nx\n' \
  | cargo run -q --bin pampa -- -t qmd | cargo run -q --bin pampa -- -t json | jq -c .meta
{"description":{"c":[{"c":"Hello","s":7,"t":"Str"}],"s":4,"t":"MetaInlines"}}
   # author dropped, no diagnostic

$ printf -- '---\ndescription: |\n  Hello —\n  world.\n---\n\nx\n' \
  | cargo run -q --bin pampa -- -t qmd | cargo run -q --bin pampa --
Error: [Q-0-99] Failed to parse YAML frontmatter: Parse error: while scanning a
quoted scalar, found unexpected end of stream at byte 13 line 1 column 14

$ printf -- '---\ndescription: "a --- b"\nauthor: Z\n---\n\nx\n' | pandoc -t json | jq -c '.meta|keys'
["author","description"]        # Pandoc 3.9.0.2 is fine
```

Scanner behaviour also checked directly: a closing `---   ` with trailing
spaces and a CRLF document both parse with all keys today, so the new
extractor must keep accepting those shapes.

## Implementation notes (2026-09-10)

What was built differs from the Option A sketch in two places, both forced
by how tree-sitter's `mark_end` works, and a few consequences are worth
knowing about.

**Four tokens, not three.** The scanner cannot rewind, and it must decide
"metadata or thematic break" *after* looking ahead for a closing line. The
only position at which both outcomes can share one `mark_end` is the end of
the opening `---` (plus trailing blanks) — the same span a thematic break
token takes. So `_minus_metadata_start` is the bare opening `---`, and a
dedicated `_minus_metadata_open_newline` token consumes its line break
(bypassing the block-structure line-ending machinery, which would otherwise
try to match open containers against the YAML lines). The body follows,
then `_minus_metadata_end` (the closing `---` plus blanks), then the
grammar's ordinary `choice($._newline, $._eof)` — the same tail as
`pandoc_horizontal_rule`, which is also what makes a closing `---` at EOF
with no newline valid. Interior tokens are gated by a new scanner state bit
`STATE_IN_MINUS_METADATA` (set by START, cleared by END, serialized with the
rest of `state`) so they cannot fire during error recovery, when tree-sitter
marks every external token valid.

**Tree shape.** `(metadata body: (yaml))`, with `yaml` spanning exactly the
lines between the delimiters (line-break inclusive at the end, zero-width
for `---\n---`). Inside a container the closing `_newline` now yields a
`(block_continuation)` child of `metadata`, exactly as `pandoc_paragraph`
already does in the same position — one corpus expectation (`div.txt: 2`)
changed for that reason. Nine pre-existing `(metadata)` expectations gained
the child; five new GH #671 cases were added to `test/corpus/metadata.txt`.
Do **not** use `tree-sitter test -u` to refresh this corpus: it rewrites
every file's indentation and drops `:skip` tests.

**Carrier contract.** `RawBlock { format: "quarto_minus_metadata" }` now
carries the YAML body as `text` and the body's range as `source_info`;
`rawblock_to_config_value` parses `text` directly and
`extract_between_delimiters` is gone. Consequences:
- The body is no longer `.trim()`med. Resolved key/value offsets are
  unchanged; the metadata's overall span now ends after the final line
  break (e.g. `4..30` instead of `4..29`), and the source-info chain is one
  layer shorter (an `Original` on the body instead of `Original` on the
  whole block + `Substring`). This shifted every `s` index in the 20
  `ts-packages/annotated-qmd/examples/*.json` fixtures (regenerated with the
  documented loop; verified that every non-`astContext` diff is an `s`
  renumbering) and 4 insta snapshots under `crates/pampa/snapshots/json/`.
- `Block::BlockMetadata` for a lexical (`_scope: lexical`) block now spans
  the YAML body rather than the delimited block. Carrying both ranges would
  mean replacing the RawBlock carrier with direct `BlockMetadata`
  construction at tree-conversion time — a reasonable follow-up, not done
  here. Flagged for the user.
- Indented frontmatter (`---\n  a: 1\nb: 2`) that only parsed because of
  the old trim now fails like it does in Pandoc.

**Parse-error table.** Regenerating `parser.c` renumbers LR states, and
`crates/pampa/resources/error-corpus/_autogen-table.json` is keyed by them;
two `meta.rs` unit tests failed until the table was rebuilt with
`crates/pampa/scripts/build_error_table.ts` (deno; the copy under
`crates/quarto-parse-errors/scripts/` is bit-rotted — `qmdFiles` undefined).
Only the table changed; the generated case files were byte-identical.

**Tests.** The three file-based tests in `test_meta.rs` used to hand the
*entire* `.qmd` (delimiters, body and all) to `rawblock_to_config_value` and
relied on the split; they now go through `readers::qmd::read`. Hand-built
fixtures in `test_rawblock_to_config_value.rs` and
`test_yaml_tag_regression.rs` dropped their `---` lines.

**Tooling trap.** `tree-sitter test`/`parse` load a compiled parser cached by
grammar *name* under `~/.cache/tree-sitter/lib/`; building any other
checkout of this grammar poisons it. Delete `markdown.dylib` there if the
corpus suddenly disagrees with `cargo` test results.

### End-to-end record

`./target/debug/pampa` on this branch (all three commands from the issue),
output inspected:

```
$ printf -- '---\ndescription: "Hello — world"\nauthor: Z\n---\n\nx\n' | pampa -t qmd | pampa -t json | jq -c .meta
{"author": Z, "description": Hello Space — Space world}        # both keys, em dash restored
$ printf -- '---\ndescription: |\n  Hello —\n  world.\n---\n\nx\n' | pampa -t qmd | pampa -t json | jq -c .meta
{"description": Hello Space — SoftBreak world.}                 # no Q-0-99
$ printf -- '---\ndescription: "a --- b"\nauthor: Z\n---\n\nx\n' | pampa -t json | jq -c .meta
{"author": Z, "description": a Space — Space b}
$ pampa -t qmd <quarto-web chrome-headless-shell/index.qmd> | pampa -t json | jq -c '.meta|keys'
["author","categories","date","description","image","image-alt","title"]
```

`target/debug/q2 render` on two fixtures (output HTML inspected):

```
$ q2 render dash.qmd      # title: "Dashes — everywhere" / description: "Hello — world" / author: Z
<title>Dashes — everywhere</title>
<meta name="description" content="Hello — world"
<meta name="author" content="Z"
$ q2 render written.qmd   # the writer's own spellings: title: Written --- form / description: "Hello ---\nworld."
<title>Written — form</title>
<meta name="description" content="Hello — world."
<meta name="author" content="Z"
```

`cargo xtask verify` (full, under Node 24 via fnm) passed end to end on 2026-09-10. Note: the first run failed in step 11 on a KaTeX `\tag` test because this checkout's `node_modules` had KaTeX 0.17.0 against a lockfile pinning 0.18.4; `npm install` fixed it with no lockfile change. Unrelated to this work.
