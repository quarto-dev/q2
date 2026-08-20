# Investigation artifacts for bd-toc-tabset-titles-zq93gjvf

Captured 2026-08-17 from the external q2-connect-docs repo
(`llms-info/repros/toc-tabset-titles/` and `docs-quarto-1/_site/`),
copied here per the external-fixtures policy so the record survives
without that checkout.

- `index.qmd`, `_quarto.yml` — minimal repro: a doc with 2 real headings
  wrapping a 2-tab `panel-tabset`. Q1 TOC: 2 entries; q2 TOC today: 4.
- `q1-target-markup.html` — the exact Q1-rendered TOC `<nav>` and tabset
  markup (nav-tabs + tab-content + tab-pane, with ids/aria). **This is
  the markup contract for the resolve transform** — match the committed
  render, not a reading of Q1's Lua.
- `q1-tabsets-sync-reference.js` — Q1's grouped-tabset sync module
  (`site_libs/quarto-html/tabsets/tabsets.js`, 95 lines). ES module whose
  `init()` Q1 calls from quarto.js; the q2 port should self-initialize
  (mirror `resources/js/clipboard/code-copy-init.js`).

Plan: `../2026-08-17-tabset-panel-tabset.md`
