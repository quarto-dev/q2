# Investigation artifacts — bd-include-in-code-block-f8mvtczn

## `repro/`

Self-contained copy of the repro the strand points at (the original lives
in a local-only checkout, `~/repos/github/cscheid/q2-connect-docs/llms-info/
repros/include-in-code-block/`, so it is duplicated here to keep the
record reproducible from this repo alone).

- `index.qmd` — a `{{< include app.py >}}` alone inside a
  ```` ```{.python filename="app.py"} ```` fence, plus a top-level
  include of the same file as a **control**.
- `app.py` — three-line include target.
- `_quarto.yml` — minimal website project.

Run:

```bash
cargo run --bin q2 -- render claude-notes/plans/include-in-code-block-investigation/repro
```

## `observed-at-head.html`

The rendered fence, extracted from `_site/index.html` at
`main` @ `bcdbce6b` (2026-08-10):

```html
<pre class="sourceCode python" data-filename="app.py"><code class="sourceCode python">?<span class="hl-variable">include</span></code></pre>
```

Byte-identical to the symptom recorded in the strand (which observed
0.16.0 and re-verified at `b2b6100c`). The `Q-17-4` warning also fires,
pointing at lines 8–10 with the hint "Put the include shortcode in its
own paragraph, surrounded by blank lines".

The control include at top level expands correctly, confirming the
failure is specific to the code-fence position.

`_site/` and `.quarto/` are deleted after rendering — do not commit
build output.
