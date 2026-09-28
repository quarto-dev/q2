# Investigation notes: bd-l6eh1635 BibTeX support

Empirical evidence backing the design decisions in
`../2026-09-28-bibtex-citeproc-support.md`. None of this touched the repo or the
existing spike — probes were run in a throwaway scratchpad Cargo project and via direct
`pandoc` CLI invocations.

## 1. `biblatex::Person::parse` and institutional authors

Throwaway probe (`biblatex = "0.11"`, no other deps):

```rust
use biblatex::{Bibliography, Person};

fn main() {
    let src = r#"@misc{org1, author = {{World Health Organization}}, title = {Report}}
@misc{org2, author = {World Health Organization}, title = {Report}}
@misc{ind1, author = {Donald E. Knuth}, title = {Report}}
"#;
    let bib = Bibliography::parse(src).unwrap();
    for key in ["org1", "org2", "ind1"] {
        let entry = bib.get(key).unwrap();
        let people: Vec<Person> = entry.author().unwrap();
        println!("{key}: {:?}", people);
    }
}
```

Output:

```
org1: [Person { name: "World Health Organization", given_name: "", prefix: "", suffix: "" }]
org2: [Person { name: "Organization", given_name: "World Health", prefix: "", suffix: "" }]
ind1: [Person { name: "Knuth", given_name: "Donald E.", prefix: "", suffix: "" }]
```

`org1` (double-braced, the real-world convention institutional entries use) is parsed
correctly as a single unit. `org2` (single/mandatory bracing only) gets split like an
ordinary personal name — expected, since single bracing carries no protection signal in
the BibTeX format itself; this is a known, universal BibTeX ambiguity, not something we
need to solve.

The critical finding is that `org1`'s *output shape* — `Person { name: "...",
given_name: "" }` — is indistinguishable from a genuine lone-mononym individual. Second
probe, confirming this and checking whether the raw chunk data preserves the missing
signal:

```rust
use biblatex::{Bibliography, ChunksExt, Person};

fn main() {
    let src = r#"@misc{mix, author = {Jean van der Waals and {World Health Organization} and Doe, Jr., Jane}, title = {Report}}
@misc{single_only_family, author = {Voltaire}, title = {Report}}
"#;
    let bib = Bibliography::parse(src).unwrap();
    for key in ["mix", "single_only_family"] {
        let entry = bib.get(key).unwrap();
        let chunks = entry.get("author").unwrap();
        println!("{key} raw chunks: {:?}", chunks.iter().map(|s| &s.v).collect::<Vec<_>>());
        let people: Vec<Person> = entry.author().unwrap();
        println!("{key} parsed: {:?}", people);
    }
}
```

Output:

```
mix raw chunks: [Normal("Jean van der Waals and "), Verbatim("World Health Organization"), Normal(" and Doe, Jr., Jane")]
mix parsed: [Person { name: "Waals", given_name: "Jean", prefix: "van der", suffix: "" },
             Person { name: "World Health Organization", given_name: "", prefix: "", suffix: "" },
             Person { name: "Doe", given_name: "Jane", prefix: "", suffix: "Jr." }]

single_only_family raw chunks: [Normal("Voltaire")]
single_only_family parsed: [Person { name: "Voltaire", given_name: "", prefix: "", suffix: "" }]
```

Confirms the fix approach: `Entry::get("author") -> ChunksRef` is public and exposes
`Chunk::Verbatim` vs `Chunk::Normal` per raw chunk. For "mix," the institution's raw
chunk is `Verbatim("World Health Organization")`, exactly matching that parsed
`Person.name`. For "Voltaire," the only raw chunk is `Normal("Voltaire")` — no Verbatim
match, so it correctly stays a plain family-only individual. Matching a parsed
`Person.name`+empty-`given_name` against a same-text `Chunk::Verbatim` in the raw field
is therefore a safe, precise signal, without needing `biblatex`'s private
`split_token_lists_with_kw` (confirmed `pub(crate)`, not usable from outside the crate).

## 2. Pandoc's BibTeX → CSL-JSON title case-folding

Direct comparison, `pandoc -f bibtex -t csljson`:

```
title = {Proceedings of {NASA} Systems}          -> "Proceedings of NASA systems"
title = {An {AI} Study of {TeX} and \textit{Biology}} -> "An AI study of TeX and\ntextitBiology"
title = {MY TITLE about {NASA} and {Quarto}}      -> "MY TITLE about NASA and Quarto"
```

Two things confirmed: (a) ordinary Title-Case words get sentence-cased ("Systems" →
"systems") but ALL-CAPS words don't ("MY"/"TITLE" untouched even though not additionally
braced beyond the field's own delimiter); (b) arbitrary embedded LaTeX commands
(`\textit{...}`) are not handled cleanly even by Pandoc itself — leaked through as
mangled text. (b) is why "full LaTeX markup interpretation" is an explicitly deferred,
documented limitation rather than a v1 requirement.

Traced (a) to primary source rather than inferring from output alone —
`citeproc`'s `src/Citeproc/CaseTransform.hs` (fetched directly via `curl` from
`raw.githubusercontent.com/jgm/citeproc/master/...`, not summarized):

```haskell
withSentenceCase = CaseTransformer go
 where
  go mblang st chunk
     | isCapitalized chunk
     , not (st == Start || st == StartSentence)
       = Unicode.toLower mblang chunk
     | isCapitalized chunk || T.all isLower chunk
     , st == Start || st == StartSentence
       = capitalizeText mblang $ Unicode.toLower mblang chunk
     | otherwise = chunk

isCapitalized t = case T.uncons t of
    Just (c, t') -> isUpper c && T.all isLower t'
    _ -> False
```

Runs per word. Only the first clause ever lowercases anything, and only when
`isCapitalized` (exactly one leading capital, rest lowercase) — "Systems" matches, "MY"/
"TITLE" don't (second uppercase letter fails the `T.all isLower t'` check), so they fall
to `otherwise = chunk` untouched. Not deliberate acronym detection — an emergent
side effect of a narrow predicate.

Separately, explicit `{braces}` are a deliberate protection mechanism, from Pandoc's own
`src/Text/Pandoc/Citeproc/BibTeX.hs` (also fetched directly via `curl`):

```haskell
protectCase :: (Inlines -> Inlines) -> (Inlines -> Inlines)
protectCase f = Walk.walk unprotect . f . Walk.walk protect
 where
  protect (Span ("",[],[]) xs) = Span ("",["nocase"],[]) xs
  unprotect (Span ("",["nocase"],[]) xs)
    | hasLowercaseWord xs = Span ("",["nocase"],[]) xs
    | otherwise           = Span ("",[],[]) xs
```

Every literal `{...}` group becomes a `nocase`-classed span before the case transform
runs; case-transform code skips a `nocase` span's contents entirely, regardless of
shape.

Confirmed our own renderer already implements the consuming side (grep,
`crates/quarto-citeproc/src/output.rs`): it already has `nocase`/`nodecoration`
span-protection logic and already parses literal `<span class="nocase">...</span>`
markup embedded in CSL-JSON string fields. So the ingestion-side fix is: reimplement the
`isCapitalized` word rule for unbraced text, wrap `Chunk::Verbatim` spans as
`<span class="nocase">`, and rely on the renderer's existing support — not new rendering
work.

## 3. `genre` field — confirmed no core-model change needed

`rg -n 'genre' crates/quarto-citeproc/src` returned nothing — no dedicated field. But
`crates/quarto-citeproc/src/reference.rs`'s generic variable lookup (used for e.g.
`citation-label`) falls through to `self.other.get(name)` for any unrecognized variable
name, so `reference.other.insert("genre", serde_json::Value::String(...))` is
sufficient; the CSL rendering side already handles arbitrary `other` variables
generically.

## 4. `bibliography-formats.lua` (Quarto 1) — confirmed it isn't a BibTeX parser

Fetched directly from
`raw.githubusercontent.com/quarto-dev/quarto-cli/main/src/resources/filters/quarto-pre/bibliography-formats.lua`.
It only reshapes `bibliography:` metadata around `pandoc.utils.references(doc)` (for
bibliography-output-format documents) or, for Typst-native-citation output, rewrites the
bibliography path as raw Typst inline to dodge Pandoc's dot-escaping in Typst output.
It never parses `.bib`/`.json` files itself — that always happened inside Pandoc's own
bundled reader, invoked as an external subprocess by Quarto 1.

## 5. `biblatex`'s own dependency tree (wasm32 risk assessment)

`paste`, `roman-numerals-rs`, `strum`, `unicode-normalization`, `unscanny` — all pure
Rust, no libc/getrandom/IO bindings observed in their `Cargo.toml`s. Inferred low risk
for `wasm32-unknown-unknown`, but not yet confirmed by an actual build — that's Phase 3
of the plan, not assumed here.
