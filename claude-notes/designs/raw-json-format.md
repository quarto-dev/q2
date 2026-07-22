# The pampa raw-json format

**Status:** Active (version 1). Shipped 2026-07-17 (bd-en2hvrwn, GH #11).
**Entry points:** `crates/pampa/src/writers/raw_json.rs`,
`crates/pampa/src/readers/raw_json.rs` (thin wrappers over the shared JSON
machinery in `writers/json.rs` / `readers/json.rs` with the raw mode flag).
**CLI:** `pampa -t raw-json` / `pampa -f raw-json`.
**Plan/history:** `claude-notes/plans/2026-07-17-raw-json-format.md`.

## Contract

> `raw_json::write` followed by `raw_json::read` is the **identity**
> (structural equality, `PartialEq`) on the pampa AST — `Pandoc` (blocks +
> full `ConfigValue` meta) and the roundtrip-relevant `ASTContext`
> projection — for every AST the rest of pampa can produce.

What is explicitly promised and not promised:

- **Promised:** every AST extension roundtrips (standalone `Inline::Attr`,
  `NoteReference`, `Insert`/`Delete`/`Highlight`/`EditComment`,
  `Shortcode`, `CaptionBlock`, `BlockMetadata`, note definitions, custom
  nodes); metadata keeps `Path`/`Glob`/`Expr` kinds, merge ops, scalar
  YAML types, and map entry order; source-info graphs keep their
  structural sharing (pool encoding, same as `-t json`).
- **Not promised:** byte-stability of `write ∘ read ∘ write`. The reader
  rebuilds pool entries behind fresh `Arc` wrappers, and the writer's
  parent-dedup is by pointer identity, so a re-serialize may intern extra
  pool entries / shift ids. The contract is semantic identity, not
  canonical bytes. (In practice small documents come back byte-identical;
  do not rely on it.)
- **Not promised:** cross-version stability. The `version` field is a
  fail-fast guard, not a compatibility contract; raw-json is a
  q2-internal format. If we ever persist raw-json long-term, that is the
  moment to write a compatibility policy.
- **No lenient reader.** Raw-json is strict about `s:` source refs —
  every writer-produced node carries one. JSON from outside the q2
  source-tracking world goes through `-f json` (the completing reader).

## Envelope

```json
{
  "pampa-json-format": {"version": 1},
  "blocks": [ ... ],
  "meta": { "c": [...], "s": 7, "t": "MetaMap" },
  "astContext": {
    "files": [ {"line_breaks": [...], "name": "...", "total_length": N} ],
    "exampleListCounter": 1,
    "p": [ ...source-info pool... ]
  }
}
```

- **`pampa-json-format` is the first key**, always — it is the format's
  self-identification, visible in the first line / truncated previews.
- **`pandoc-api-version` is deliberately absent** so machine consumers of
  Pandoc JSON fail fast instead of half-parsing.
- Key order is a streaming constraint: `astContext` must come last
  because the source-info pool is complete only after the AST walk.
- `meta` is a single config-value node (see below), not a Pandoc-style
  object; there is no `metaTopLevelKeySources` sidecar in raw mode (key
  sources ride inline).
- Reader rejections (all `JsonReadError` variants, no Q-codes — the JSON
  reader path predates catalog integration): Pandoc-style input →
  `NotRawJson{has_pandoc_api_version: true}` ("use `-f json`"); unmarked
  input → `NotRawJson{..: false}`; unknown version →
  `UnsupportedRawJsonVersion`. Symmetrically, the Pandoc-superset reader
  rejects raw-json input with `UnexpectedRawJsonMarker` ("use
  `-f raw-json`").

## Node vocabulary (delta over the Pandoc-superset format)

Everything the `-t json` format emits is unchanged (same `t`/`c` tags,
same `s`/`a`/`targetS` sidecars, same pool codes — see
`wire-format-source-info-codes.md`, which raw-json shares). Raw mode adds:

| Tag | Shape | AST type |
|---|---|---|
| `Attr` | `{a, c: [id, classes, kvs], s, t}` | standalone `Inline::Attr` |
| `NoteReference` | `{c: id_string, s, t}` | `Inline::NoteReference` |
| `Insert` / `Delete` / `Highlight` / `EditComment` | `{a, c: [attr, [inlines]], s, t}` (Span-shaped) | CriticMarkup inlines |
| `Shortcode` | `{c: {isEscaped, keywordArgs, name, positionalArgs}, s, t}` | `Inline::Shortcode` |
| `CaptionBlock` | `{c: [inlines], s, t}` | `Block::CaptionBlock` |

Shortcode args are tagged nodes: `String`/`Number`/`Boolean` are
`{c, t}`; a nested `Shortcode` is a full `{c, s, t}` node; `KeyValue` is
`{c: [[key, arg]...], t}` with entries **sorted by key** (the underlying
`HashMap` has no stable order; sorting keeps output deterministic, and
map equality on read is order-independent). `keywordArgs` pairs keep
`LinkedHashMap` insertion order.

## Metadata encoding

`meta` and every nested value is a `{c, m?, s, t}` node:

| Tag | `c` | AST |
|---|---|---|
| `MetaString` | string | `Scalar(Yaml::String)` |
| `MetaBool` | bool | `Scalar(Yaml::Boolean)` |
| `MetaInt` | i64 | `Scalar(Yaml::Integer)` (raw only) |
| `MetaReal` | string (yaml_rust2 raw source string, byte-faithful) | `Scalar(Yaml::Real)` (raw only) |
| `MetaNull` | null | `Scalar(Yaml::Null)` (raw only) |
| `MetaPath` / `MetaGlob` / `MetaExpr` | string | `Path` / `Glob` / `Expr` (raw only) |
| `MetaInlines` / `MetaBlocks` | AST arrays | `PandocInlines` / `PandocBlocks` |
| `MetaList` | array of nodes | `Array` |
| `MetaMap` | array of `{key, key_source, value}` entries, **source order** | `Map` |

- `m` carries a non-default merge op (`"prefer"`; the default `"concat"`
  is omitted). Only raw mode emits it; the reader accepts it whenever
  present.
- `MetaMap` uses an array of entries (not a JSON object) because
  serde_json objects don't preserve key order on read; entry order is
  part of the contract.
- A `Scalar` holding a non-scalar YAML value (Array/Hash/Alias/BadValue)
  has no faithful encoding and fails the write loudly with **Q-3-57**
  rather than degrade silently.

## ASTContext

`astContext.files` and the pool `p` are identical to the Pandoc-superset
format (writer emits, `read_ast_context` + `SourceInfoDeserializer`
restore; structural sharing preserved). Raw mode additionally carries
`exampleListCounter`. Not carried: `parent_source_info` (parse-time-only
state) and file *contents* (line-break tables only, matching `-t json`;
the contract is about the AST, not re-deriving source text).

## Versioning policy

- The version is a single integer in the marker; the reader accepts
  exactly the versions it knows (currently: 1).
- Additive, ignorable keys (a new optional sidecar) do not need a bump —
  readers ignore unknown keys, matching the existing JSON reader's
  behavior.
- Any change a version-1 reader would misread (tag shape change, meaning
  change, removal) bumps the version. Wire-shape questions for the shared
  pool codes follow `wire-format-source-info-codes.md` — raw-json is a
  second consumer of those codes and inherits that policy.

## Maintenance rule (how the roundtrip stays true)

The raw arms live inside the same exhaustive `match`es as the
Pandoc-superset arms, so **adding a new `Block`/`Inline` variant fails
compilation until its raw behavior is decided** — decide it by adding a
native raw tag (and reader arm + roundtrip test), not a desugaring. The
roundtrip corpus (`tests/integration/test_raw_json_roundtrip.rs`,
including the `tests/writers/json/*.md` sweep) is the semantic guard;
extend it with every new construct.
