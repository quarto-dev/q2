# Positron conformance fixtures (EXR-16)

Documents that q2 and Positron's Quarto parser both read, so a drift in unit
identity (content hash, cell id, label, which fences count as cells) shows up as a
failing test on one side instead of silent mismatches.

## Files

| file | written by | content |
| --- | --- | --- |
| `<name>.qmd` | the generator | the document, byte for byte (`crlf.qmd` has CRLF endings) |
| `<name>.q2.json` | the generator | every unit q2 finds: `kind`, `lang`, `index` or `span_index`, `line`, `content_hash`, `cell_id`, and for cells `label` |
| `positron-expected.json` | hand-reviewed, filled from Positron's vitest output | per case, `cells` (what Positron's parser returns: `id`, `language`, `label`, `content_hash`, `line`) and, where its ids or hashes differ from q2's, a `divergence` note saying why |

The hash rule is shared (SHA-1 over the lines joined with `\n`, 16 hex characters); the
ids are the shared format `{index}-{hash8}-{label|unlabeled}` but are **not** a shared
key, as `divergence` entries show. Matching uses the hash only. The hash vectors live in
`../records/content-hash-vectors.json`.

## Commands

q2 side (cases are the table in `tests/integration/positron_conformance.rs`):

```bash
# check
cargo nextest run -p quarto-core -E 'test(positron_conformance::)'
# regenerate .qmd and .q2.json after a deliberate change, then review the diff by hand
UPDATE_POSITRON_FIXTURES=1 cargo nextest run -p quarto-core -E 'test(positron_conformance::)'
```

The q2 test fails if `positron-expected.json` has a divergence note on a case where the
ids agree, or none on a case where they differ.

Positron side (EXR-16 spike S1, in a worktree of the fork, branch
`spike/q2-s1-conformance`; needs no build daemons):

```bash
Q2_POSITRON_FIXTURES=<abs path to this directory> \
  npx vitest --run src/vs/workbench/contrib/positronQuarto/test/browser/quartoQ2Conformance.vitest.ts
# to refresh positron-expected.json, also set Q2_POSITRON_OBSERVED_OUT=<file> and
# copy the new `cells` over after reviewing each change
```

Last observed against Positron `f854bb8e66` (2026-11-01): hashes agree on every cell both
parsers see; ids differ in 11 of 20 cases (see the `divergence` notes).

Nothing here is sent to the Positron project.
