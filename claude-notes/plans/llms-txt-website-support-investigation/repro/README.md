# llms-txt-unimplemented

`website.llms-txt: true` is silently ignored by q2: no `llms.txt`, no
per-page `*.llms.md` companions, no warning.

Origin strand: `br-llms-txt-unimplemented-qmgjbb46` in the
q2-connect-docs skein.

## Run

```sh
q2 render .
find _site -name 'llms.txt' -o -name '*.llms.md'
```

## Expected (Quarto 1, dev 99.9.9)

`quarto render .` emits, alongside the HTML output:

- `_site/llms.txt` — a markdown index of every page:

  ```markdown
  # llms-txt repro

  ## Pages

  - [About](about.llms.md)
  - [Home](index.llms.md)
  ```

- `_site/index.llms.md` and `_site/about.llms.md` — plain-markdown
  renderings of each page (main content extracted from the rendered
  HTML, converted back to markdown).

Q1's implementation lives in
`quarto-cli/src/project/types/website/website-llms.ts`
(`llmsHtmlFinalizer`).

## Actual (q2 0.21.0)

`q2 render .` produces only the HTML output. The `llms-txt` key is
accepted without warning and dropped. Grepping q2's `crates/` for
`llms` finds only the input side of the convention (project discovery
excluding `*.llms.md` as agent-instruction files, in
`crates/quarto-core/src/project/discovery.rs`) — there is no output
implementation.

## Real-world hit

The Connect docs set `website.llms-txt: true` in `_quarto.yml`. Q1's
`_site` ships `llms.txt` plus 348 `*.llms.md` companions; the q2 port
ships none. The docs landing page (`index.md`, a product card) links
to `llms.txt`, so the ported site 404s on its own front page.
