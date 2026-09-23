# Upstream spec inputs

`mitex-default-spec.json` is mitex's default command specification
(<https://github.com/mitex-rs/mitex>, Apache-2.0), dumped from the prebuilt
`default.rkyv` in the `mitex-rs/artifacts` submodule (mitex `985d8e7`,
artifacts `9eb762a`, spec generated from `packages/mitex/specs/latex/standard.typ`
by `typst query`). Shape: `{"commands": {name: CommandSpecItem}}`, keys sorted,
995 entries (968 commands, 27 environments).

It is an **input**, not the spec quarto-math runs on:

- the vendored mitex test suites under `../../vendor/*/tests/` load it in
  place of `mitex_spec_gen::DEFAULT_SPEC`;
- the q2-owned spec (`../commands.json`, Phase 1) is generated from it plus
  hand-written OMML semantics, so a mitex spec bump is a re-dump followed by a
  regeneration, never a hand edit of this file.

Regenerate with `claude-notes/research/2026-09-21-quarto-math-probes/spec-dump/`.
An entry's `alias` is the Typst symbol or function mitex maps the command to;
`null` means "same name as the command" (e.g. `alpha`).
