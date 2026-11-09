---
title: 'OPR-0d: could the execution-records source list be a metadata key?'
date: 2026-11-08
description: 'Research for Gordon''s question R5: whether the source list of --execution-records could be a document or project metadata key instead of, or beside, a command-line flag. Findings on hashing, hub trust, path resolution, machine-local sources, convert, conflict checks and per-command defaults, three options with their effect on OPR-5 and OPR-6, and a recommendation. No product code changed.'
---

**Plan:** [OPR-0d](../plans/2026-11-08-opr-0d-metadata-key-research.md) · **Epic:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md) ·
**Decides:** the "R5" box in [OPR-5](../plans/2026-11-08-opr-5-execution-records-definition.md). Gordon\'s decision is recorded below and in OPR-5; flipping OPR-5 to `ready` is still his step.

Evidence marks: **(ran)** I executed it, **(read)** I read the code at the named symbol, **(not done)** I did not check it.
Paths are in this checkout, on top of `feature/one-presenter`.

## Decision (Gordon, 2026-11-09): move the option into metadata

After reading this note Gordon chose to **move** the source list into metadata and **remove the command-line option
entirely**: there is no `--records` and no `--execution-records` on any command. The rest of the note is the evidence;
where it recommends keeping the flag (see "Recommendation" below), that recommendation is **superseded**. The decisions,
which OPR-5, OPR-6, RFU-1 and the docs plans now carry:

1. **The key.** `execute: record-sources:`, a **comma-separated scalar string** (`positron,engine`), not a YAML list.
   A list from `-M` or the provider overlay would append to the document\'s list, because the runtime layer is JSON
   and cannot carry `!prefer` (read: `json_to_config_value` builds nodes with the default merge op, which concatenates
   arrays). A scalar is replaced, last layer wins. **Known limitation, a better solution is wanted later:** either the
   provider reads the runtime layer directly, or `json_to_config_value` learns a way to say "prefer".
2. **No option, `-M` instead.** The command-line spelling is `-M 'execute:{record-sources: positron,engine}'` (dotted
   keys are not expanded by `-M`; Gordon accepts the longer spelling). `q2 convert` gains `-M`.
3. **Entries.** The keywords `engine`, `q2`, `positron`, `cache`, and a **directory inside the project** (v1
   restriction): a relative path resolves against the declaring layer\'s `base_dir`, a leading `/` is the project root,
   and a path that resolves outside the project is an error. No `~`, no absolute paths, no existence fallback.
4. **Per command.** Each command has its own allowed values and default (preview `cache,q2,engine`, render `engine`,
   convert `q2,engine`, provide-hub `engine`). A valid entry the command cannot use (`cache` on render) is **ignored
   with an info line** once per run; an unknown or malformed entry (`postiron`) is an **error**.
5. **Per document.** The list is resolved per document, which supersedes D6\'s "one list for all documents".
6. **One early reader.** The list is needed before any pipeline runs, the same problem as the output format. It is read
   by a new `resolve_record_sources`, built on `metadata_layers::collect_metadata_layers` (the collector the format
   resolver `format_resolution::resolve_document_format` uses), front matter only, so `render`, `preview`, `convert`
   and the provider all call it. A small "highest layer that sets this key path" helper joins the collector.
7. **Conflicts.** `--no-execute` and `execute: enabled: false` veto `engine` quietly (the entry is ignored).
   `--replay` and `QUARTO_REPLAY` win over a committed key, with an info line, and are not an error.
8. **Untrusted authors get a ceiling** (`provide-hub` and a `--share` preview; no other command). The allowed set is the
   host\'s `-M` value, or the command\'s default when there is none. A guest document may **reorder or narrow** it and may
   not add to it. An entry outside the set is ignored with a **warning-level** diagnostic. In the provider the warning
   goes to the operator\'s log (and is appended to `last_error` when the run fails), because the provider has no route
   for a warning to a client: strand `bd-ia3idnvk` tracks a client-visible `warnings` field on the capture sidecar. In
   the `--share` preview the warning goes through the page diagnostic sink. Why `--share` and not only `--allow-edit`:
   execution reads the host\'s disk, which a guest reaches only with `--allow-edit`, but the preview\'s render routes
   (`POST /api/preview/render` and the downloads) take a guest-supplied `content` and `metadata` (the runtime layer,
   the highest precedence), and OPR-6 makes `render_to_file` read the list.
9. **Hash.** The key is an `execute:` key, so it counts in every engine\'s document hash unless excluded. It is excluded
   in the three declarations (Jupyter, knitr with a `KNITR_HASH_VERSION` bump, Julia in `record-wire.ts` after OPR-3).
   Record compatibility is not a concern for this epic.

**Corrections to the text below.** The "Machine-local sources" section says a committed `positron` or `q2` entry "breaks
another machine". It does not: a source that finds nothing returns `Nothing` and the walk moves to the next entry
(`resolve_records`), so a committed `positron,engine` falls through to `engine` on a machine without Positron. The two
real costs are that the output then depends on who runs it, and that a directory entry is a startup error where it is
missing. The "Recommendation" section recommended the flag; it is superseded by the decision above.

## Short answer

A metadata key is **possible**, and it is not blocked by hashing if it sits at the top level of the metadata (outside
`execute:`). The reasons against making it the *only* mechanism are about who can set it and where the sources live,
not about plumbing:

- The sources people actually need a list for are machine-local (`positron`, a directory). A committed file cannot
  say them portably, and the metadata path rule cannot even spell them.
- In the hub provider, a guest's project metadata takes effect on the host, and the imported sources need no consent.
  A key would hand a guest a way to point the host at a host directory. It needs a new trust rule first.
- `convert` has no `-M`.

The recommendation (end of note) is the flag first (OPR-5 as written), with a narrow key (`engine` and `q2` only,
flag wins) left as an additive follow-up.

## Hash effect

**Question.** Would adding a key change the document hash, and which exclusion points would it need?

**Ran.** A scratch test (not committed; reverted) called `hash_document` for each engine's declaration, with and without
the key, over an empty unit list. `HashDefinition::conservative("julia")` stands in for Julia's declaration, which lives
in TypeScript and has empty `presentationExecuteKeys` too (read, `record-wire.ts`, `JULIA_HASH_DEFINITION`).

| metadata added | jupyter | knitr | julia-like |
|---|---|---|---|
| `execute: {execution-records: engine}` | changes | changes | changes |
| `execute: {record: true}` | changes | changes | changes |
| top-level `record: true` | same | same | same |
| `execute: {echo: false}` (control) | same (excluded) | changes | changes |

**Read.** `hash_document` (`crates/quarto-core/src/record/exec_hash.rs`) covers only `engine`, `execute-dir`, `params`,
the engine's `document_keys`, the `execute:` map minus the engine's `presentation_execute_keys`, and the unit hashes.
So:

- A key **inside `execute:`** counts for every engine until it is listed as a presentation key. The three exclusion
  points would be Jupyter's `DISPLAY_ONLY_KEYS` (`engine/jupyter/mod.rs`, which also feeds `presentation_execute_keys`),
  knitr's `presentation_execute_keys` in `knitr_hash_definition` (`engine/knitr/hash_rule.rs`; empty today, and
  `KNITR_HASH_VERSION` is 2, so a bump is due), and Julia's `presentationExecuteKeys` in `record-wire.ts` (version 1).
  A bump invalidates every existing record for that engine.
- A key **at the top level** (for example `execution-records:`) is in no hash, because top-level keys count only when an
  engine lists them in `document_keys`. This needs no exclusion and no version bump.
- **`execute: record: true` itself enters the hash (ran), on all three engines.** The special case at
  `exec_hash.rs:~292` filters `record` out of `document_keys`, which are top-level keys; it never touches the
  `execute:` map. The doc comment ("core always excludes `record`") describes the top-level key only. I grepped the
  engine and stage code for a caller that strips `execute.record` before hashing and found none, but I did not run a
  full render to confirm **(not done)**. If it holds, toggling `record: true` marks existing records stale. That is a
  finding for the RIU owners, outside this plan; I have not opened a strand.

Whether the source list *should* be in the hash: no. It chooses where a record comes from, not what code ran. Hashing
it would stale a record by changing the list that finds it.

## Hub trust

**Read** (`crates/quarto-hub-provider/src/execute.rs`):

- `run_and_store` materializes the **guest's whole project** into a temp dir and calls `ProjectContext::discover`
  on it, so the guest's `_quarto.yml`, `_metadata.yml` and front matter all take effect on the host.
- Consent (`self.consent.review`) is consulted in `run_and_store` only, before an engine runs.
- `publish_record_source` states that imported sources (`positron`, a directory) "need no consent": they never call
  an engine, only read a cache and publish it into the hub.
- The provider's list today comes from the host's own command line (`--records`), and `-M` there is a host-controlled
  overlay.

**What a key would let a guest do.** If a guest's metadata could name `positron` or a directory, the host would scan
its own Positron cache directories (`default_directories`) or that directory (`locate_cache`) when a guest asks for
an execution, and publish what matched into the hub, where every guest sees it. `locate_cache` matches by content
hash of the cells, so what leaks is limited to cache files whose cells hash like the guest's cells; a guest who can
copy a document the host owns can still choose it. I did not try this **(not done)**; it is an argument from the code.
A `engine` or `q2` entry gives a guest nothing new: engine runs stay consent-gated, and the `q2` source reads
`.quarto/execution-records/<doc>.ipynb` from the guest's own tree.

**A key is not safe in the provider without a new rule.** Two rules would make it safe:

1. Honor the key only from host-controlled metadata (the `-M` overlay), never from the guest's project files.
2. Or honor only `engine` and `q2` from project files and refuse everything else with an error naming the entry.

The preview has a smaller version of this: with `--share --allow-edit` guests can edit files that reach the host's
disk, and a key in front matter would then steer the host's preview. I did not trace how edits reach the metadata
reader **(not done)**.

## Path resolution

**Read** (`claude-notes/designs/path-resolution-model.md`, `config_paths.rs`).

- As a **key**, a directory entry resolves against the file that declared it, and a leading `/` means the **project
  root**, "never an OS-absolute filesystem path". `resolve_config_path` is the mechanism, and a consumer must not join
  the string onto a base it picked.
- As a **flag**, `RecordSource::from_str` treats `/`, `./`, `../` and `~` as directory paths and `check_directory`
  resolves them against the working directory (read, `record/sources.rs`).

So the two grammars do not carry over. `/abs/dir` and `~/dir` mean something else as a key, and `~` is not a path
in the config model at all. A Positron cache directory lives in an application storage folder outside the project, so
a project-relative directory can rarely name one.

The `config-path-base` lint covers only `crates/quarto-core/src/{stage,transforms}/` (read, its module doc). A
resolver in `record/sources.rs` or the CLI would not trip it, but the contract still applies, so a key would need
`resolve_config_path` with a `PathCtx` there by review, not by lint.

## Machine-local sources

| entry | meaning | fine in a committed file? |
|---|---|---|
| `engine` | run the code | yes, portable |
| `q2` | `.quarto/execution-records/` copy | partly: the directory is usually gitignored, so CI would not have it, but the entry is portable |
| `cache` | the preview's capture cache on this machine | no, and render refuses it (D6) |
| `positron` | this user's Positron cache, if Positron is installed | no, per user and per OS |
| a directory | a path on this machine | no, unless project-relative and committed |

The entries a team would commit (`engine`, `q2`) are the ones that already work by default or add little. The ones
that make the list worth having are the ones a committed file gets wrong.

## `convert`

`q2 convert` (RFU-1) has no `-M` today (the plan's claim, and RFU-1 gives it `--execution-records`; I did not read
RFU-1's final shape **(not done)**). Options: add `-M` to `convert` (one more flag for one more command, and its
metadata would be the input document's own), or keep the flag there regardless. Positron conversion is machine-local
by nature, so the flag stays the natural fit, and with a key-only design `convert` would still need an escape hatch.

## Conflicts

- **`--no-execute` plus `engine`.** `--no-execute` is a clap flag that becomes `ExecutionPolicy::None`
  (`render_execution_policy`, `main.rs`). D6 makes it veto `engine`. With a flag list this is one check at startup.
  With a key it must run after metadata is merged, per document, so the diagnostic arrives later and could differ
  by document.
- **`--replay`.** The flag is `Option<String>` in clap, and `QUARTO_REPLAY` is read in `commands/render.rs`
  (`load_replay_captures`), not by clap, so D6's check is already a runtime check. As a key, a project that commits
  `execution-records: q2` would make every `--replay` run an error, unless replay quietly wins. That decision would
  be new.
- **One list for all documents (D6).** A key can differ per document. D6 chose one list, walked per document. A
  project-level key fits D6; a document-level key breaks it, so a key would have to be project-level, or D6 changes.

## Per-command defaults

**Read.** There is no `render:` or `preview:` metadata section for commands. Metadata that commands read today is
`preview.engine` (the engine *policy*, read only by `quarto-preview`, `config.rs`) and `project.preview` (Q1's static
preview block: port, host, browser, watch-inputs, navigate). A `render:` key does not exist. So the preview already
splits as a metadata key for policy and a flag for sources, which is the shape of option (c). Per-command defaults from
metadata would need a new section with its own reader, and a decision for each command.

## Options

**(a) Flag only.** `--execution-records` on `render`, `preview` (including `--static`), `provide-hub` and `convert`,
as OPR-5 and OPR-6 are written. Good: matches D6, one list per run, startup validation, no hash or trust work,
works for machine-local sources. Bad: a team cannot commit a default; every command line repeats it.

**(b) Metadata only.** A top-level key, resolved per project or document, and no flag. Good: `-M` gives it for free on
`render`, `preview` and `provide-hub`. Bad: cannot spell `~` or absolute directories; needs the hub trust rule; the
`convert` gap; conflict checks move to per-document time; `-M` values do not resolve paths relative to the document
(per the `-M` help text), so `-M execution-records:./dir` would resolve against nothing defined.

**(c) Mix.** Key for `engine` and `q2` (project-level, top-level key so no hash work), flag for machine-local sources
and paths, flag wins. Good: covers the committed-config case and the machine-local case. Bad: two mechanisms, a
precedence rule, the trust rule is still needed, and the key's value set must be validated separately.

### Effect on OPR-5 and OPR-6

| option | OPR-5 | OPR-6 |
|---|---|---|
| (a) flag only | none beyond today's plan: the shared option group, three resolvers into one, the rename | none beyond today's plan |
| (b) metadata only | rewrite: no flag; a metadata reader in `quarto-core` with `resolve_config_path`; drop `~` and absolute directories; provider trust rule and tests; `convert` gets `-M` or a flag; rename becomes a key name; TypeScript e2e files change to `-M` | rewrite: the per-document seam reads the key; `--no-execute` and `--replay` checks move per document; D6's "one list" restated |
| (c) mix | the (a) work, plus a key reader for `engine` and `q2`, a precedence rule (flag wins), the provider trust rule and its tests | the (a) work, plus the seam merges flag over key; conflict checks run on the merged list |

## Recommendation (superseded by the decision at the top)

**(a) for OPR-5 and OPR-6 now; (c) as an additive follow-up if a team asks for committed defaults.**

1. The lists that need a mechanism are machine-local, and the metadata path rule cannot express them.
2. A key in the hub provider needs a new trust rule before it ships; a flag already sits on the host's side of that line.
3. D6's one list and its conflict checks (`--no-execute`, `--replay`) are one-time startup checks for a flag and
   per-document checks for a key.
4. It is cheap to add later and costly to take back: a top-level `execution-records:` restricted to `engine` and `q2`,
   project-level only, flag wins, needs no hash change and no change to OPR-5's grammar, whereas a key shipped first
   and retired later is a compatibility break.
5. If Carlos asks "why not a YAML field": the preview already treats policy (`preview.engine`) as metadata and sources
   as a flag, and the reasons above are the answer to the rest.

If Gordon picks (b) or (c), these are the answers this note could not settle and OPR-5 would need first: whether
the key is project-level or per document (D6), whether `--replay` beats a committed key, and which of the two trust
rules the provider takes.

Not done: no end-to-end render with the key (there is no key to render), no test of the provider leak, no read of RFU-1's
final `convert` shape, no check of how preview guest edits reach the metadata reader.
