require "yaml"
cases = YAML.load_file("crates/quarto-core/tests/integration/engine_resolution_corpus/cases.yaml")
def fm(c)
  parts = []
  parts << "project: #{c['project'].strip.gsub("\n", '; ')}" if c['project'] && !c['project'].strip.empty?
  f = c['front_matter'].to_s.strip
  parts << (f.empty? ? "(none)" : f.gsub("\n", '; '))
  parts.map { |p| "`#{p}`" }.join(" + ")
end
def esc(t)
  t.gsub("{", "\\{").gsub("}", "\\}").gsub("[", "\\[").gsub("]", "\\]")
end
def out(o)
  seq = o['sequence'].empty? ? "none" : o['sequence'].join(", ")
  own = o['ownership'].empty? ? "none" : o['ownership'].map { |k, v| "#{k}=#{v}" }.join(", ")
  "`#{seq}` / `#{own}`"
end
pass = cases.count { |c| c['status'] == 'pass' }
pend = cases.size - pass
puts <<~HEAD
---
title: 'Engine resolution corpus and design review (bd-yt2o2ovt)'
date: 2026-11-04
description: 'Review of the Jupyter/engine interface: Q1 behavior, the final proposed interface (D2b), amendments, and a resolver-level corpus of #{cases.size} cases that is the Phase B3 test specification.'
---

# Overview

This note accompanies the plan `claude-notes/plans/2026-11-04-engine-interface-review.md`,
which holds the findings, the decision (D2b) and the open recommendations (R1
to R7). It adds the two things the review owed: the final proposed interface in
one place, and the corpus.

Labels: **RAN** means executed and the output read; **DERIVED** means read from
code or reasoned. Every number in the corpus table below is a resolver result:
the `today` column was run against main `94e1f8afe`, and the target column was
checked by running the whole corpus against a temporary prototype of D2b (a
joint T1/T2 pass, about twenty lines in `run_tiers`, reverted afterwards).

# Final proposed interface

**Selection.** A language is owned by the best claim among the engines
resolution considers. Explicitly declared engines are the ones named by
`engine:` (scalar, list or map) or by a top-level key named after a registered
engine (`jupyter:`, `knitr:`) when `engine:` is absent.

**Precedence (D2b, decided).** Jointly in T1 and T2, an explicit engine\\'s
`Fallback(p)` counts as `Primary(max(1, p))`. Candidates compare on
`(priority, class)`: a listed engine\\'s own Primary beats a listed engine\\'s
promoted Fallback, which beats an unlisted engine\\'s Primary. Candidate order
(listed first, declared order) breaks remaining ties. T3 Interop and T4
implicit Fallback are unchanged.

**Spellings.** `engine: jupyter` and `jupyter: ark` (a Q1 spelling) select
Jupyter and, after D2b, let it own the languages no other listed engine claims
specifically. `engine: [a, b]` and the map form are q2 only (Q1 rejects them).
`claims:` tables narrow or widen an engine and replace its default claims.

**Kernel (D1, with R1 and R2 pending).** Explicit name from
`engine: {jupyter: {kernel}}`, then `jupyter: X` or `jupyter.kernel`, then (R2)
`jupyter.kernelspec.name`; else the kernelspec whose `language` matches the first
executable cell (case-insensitive); else the old name table. An unknown explicit
name is a hard error. One kernel per document; R1 recommends an error when a
Jupyter-owned cell\\'s language differs from the kernel\\'s declared language.

**Layering.** Arrays from project and document layers concatenate, so D2b
makes the result independent of layer order for the cases the review found
(C51 and C52 give the same owners). `!prefer` or a scalar document `engine:`
replaces the project value.

# Amendments to the plan and the design document

- D2 is replaced by D2b: T2 is not retired; it is evaluated jointly with T1
  (plan, decided).
- `engine-resolution.md` section 2 (the sentence saying explicit `engine:` does
  not preempt division), section 4 (tier text, T1 and T2 merged into one pass),
  and the section 4.4 table change with Phase B3.
- Of the existing unit tests, only `table_fallback_is_a_real_floor` fails under
  the D2b prototype (RAN: 724 of 725 `engine::` tests pass). The tests that pin
  T2 (`test_b2_t2_explicit_fallback_wins_unclaimed_language` and the
  `[knitr, jupyter]` cases) pass unchanged.
- Phase B2 gains R1 and R2 items; Phase B3 starts with this corpus; Phase B4
  holds the diagnostics (R3, R4). R5 (claims parser) and R7 (handler
  languages) are separate strands.

# The corpus

Source of truth: `crates/quarto-core/tests/integration/engine_resolution_corpus/cases.yaml`
(this table is generated from it by `render_table.rb` in the same directory:
`ruby render_table.rb > claude-notes/research/2026-11-04-engine-resolution-corpus.md`
from the repository root),
driven by `engine_resolution_corpus.rs` (module `engine_resolution_corpus` in
`crates/quarto-core/tests/integration/main.rs`). Run with
`cargo nextest run -p quarto-core --test integration engine_resolution_corpus`.

Result on branch `braid/bd-yt2o2ovt-ark-r-kernel` (main `94e1f8afe` plus the cases added since): #{cases.size} cases, #{pass} pass, #{pend} expected-fail.
A `pending-phase4` case pins today\\'s behavior and fails once it meets the
target, so the marker gets flipped when Phase B3 lands. Registries: E1 is the
built-ins (knitr, jupyter, markdown); E2 adds a Julia extension at Primary(1);
E3 adds a marimo-style extension at Primary(1) on `python` with class `marimo`;
E3b is the same at Primary(2). Sequence order is exact for E1 and compared as a
set for E2, E3 and E3b (the registry contribution order has no public setter).

Each owner cell reads `sequence` / `language=owner`. The Q1 column says how Q1
behaved and how that was verified.

| id | declaration | cells | reg | target (D2b) | today | Q1 (verified) | verdict |
|---|---|---|---|---|---|---|---|
HEAD
cases.each do |c|
  today = c['status'] == 'pass' ? "same" : out(c['today'])
  langs = c['languages'].empty? ? "none" : c['languages'].map { |l| "`#{l}`" }.join(" ")
  q1 = esc("#{c['q1']} (#{c['q1_verified']})".gsub("|", "/"))
  puts "| #{c['id']} | #{fm(c)} | #{langs} | #{c['registry']} | #{out(c['expect'])} | #{today} | #{q1} | #{c['verdict']} |"
end
puts
puts "# Case notes"
puts
cases.each do |c|
  next unless c['notes']
  puts "- **#{c['id']}** (#{esc(c['title'].gsub(/`/, ''))}): #{esc(c['notes'])}"
end
