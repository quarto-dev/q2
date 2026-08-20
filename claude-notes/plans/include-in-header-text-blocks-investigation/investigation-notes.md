# HEAD run (2026-08-20, main @ 87c0e21a)

Invocation: `cargo run --bin q2 -- render claude-notes/plans/include-in-header-text-blocks-investigation/repro`
then `grep -o 'marker-[a-d]' _site/<file>.html`.

| file | diagnostics at HEAD | marker in output |
|---|---|---|
| index.qmd (```` ```{=html} ```` fence) | Q-5-5 Invalid include form, span = whole `text:` entry (4:3–9) | **none** |
| multi-para.qmd (two `<meta>` paragraphs) | Q-2-9 ×2 (HTML element converted to raw HTML), then Q-5-5 | **none** |
| bare-html.qmd (bare `<style>`) | Q-1-20 Failed to parse metadata value as markdown | marker-b |
| inline-raw.qmd (`` `…`{=html} ``) | silent | marker-c |

Matches the strand's table. Extra note: multi-para gets *three* warnings, the
first two of which (Q-2-9) tell the author the conversion worked — and then the
content is dropped.

Pre-flight `cargo xtask verify --skip-hub-build` passed at this HEAD
(log was under `.tmp-investigate/verify.log`, not committed).
