---
title: 'RFU-7: raw-record experiment for knitr fallback cells and marimo'
date: 2026-11-09
description: 'Pushing q2''s unfiltered execution record for a knitr document with fallback cells, and for a marimo document, into an open Positron document under Playwright. The push succeeds and matches every cell, but both shapes show raw markdown or an encoded payload instead of outputs. Recommendation: keep the two export skips.'
---

**Plan:** [RFU-7](../plans/2026-11-08-rfu-7-positron-fork.md) \
**Landing note:** [RFU-7 landing note](2026-11-09-rfu-7-landing-note.md) \
**Fork:** `/Users/gordon/src/positron-riu6-a`, branch `q2/rfu-7-demo-followups`, test `test/e2e/tests/quarto/q2-raw-record-experiment.test.ts`.

## Question

`q2 convert --to positron-ipynb` skips two shapes of cell on purpose: a **fallback cell** (a knitr chunk whose
output is finished `text/x-quarto-markdown`) and a cell in an **extension-language** document such as marimo. If
Positron showed the unfiltered record for those cells acceptably, the skips could be lifted. This run pushes the raw
record, with no export filter, into an open document and records what appears.

## Result

| Shape | Push result | What the editor showed | Mark |
|---|---|---|---|
| knitr, 5 chunks, 3 of them fallback | `ok`, 5 matched, 0 skipped | The two structured chunks showed `[1] 2` and `**KNITR-ASIS**` as text. The three fallback chunks showed q2\'s finished markdown as literal text: `::: {.cell}`, the code fence, and `![](fallback_files/figure-html/unnamed-chunk-2-1.pdf)`, with no image. | Playwright |
| marimo, 2 cells | `ok`, 2 matched, 0 skipped | Each cell showed the literal text of a raw HTML block that starts `<marimo-quarto-island data-marimo-payload="eyJ...`, a base64 payload. No rendered output. | Playwright |

Nobody watched these runs. The scan read each inline output\'s text and image count; it did not capture screenshots.

The first knitr chunk and the `results: asis` chunk are structured cells, and they behave as they do through the
normal export. The harm is confined to the cells the export skips.

## Recommendation

**Keep both skips.** The push does not fail, so nothing signals a problem, but a user who pushed these records would see
markdown source and an encoded payload where outputs belong. The skips are doing their job. To lift either one,
Positron would first need to render `text/x-quarto-markdown` (or q2 would need to convert a fallback cell to a
structured one), which is a separate piece of work for Gordon to scope.

## How to reproduce

The fixtures were rendered with the `q2` built from this branch.

### knitr fixture

`fx/knitr/fallback.qmd`. The PDF device and the replaced `output` hook are what make chunks fall back.

````text
---
title: knitr fallback cells
engine: knitr
execute:
  record: true
---

```{r}
1 + 1
```

```{r}
#| dev: pdf
plot(1:3)
```

```{r}
#| results: asis
cat("**KNITR-ASIS**\n")
```

```{r}
knitr::knit_hooks$set(output = function(x, options) paste0("```\nHOOKED: ", x, "```\n"))
```

```{r}
print("KNITR-AFTER-HOOK")
```
````

Run `q2 render fallback.qmd` (HTML). q2 reports "knitr Hook Replaced By The Document" (Q-18-13) and writes
`.quarto/execution-records/fallback.ipynb`. A `--to typst` render produces structured cells only, and `--to pdf` and
`--to latex` are not q2 formats, so HTML is the way to get fallback cells from the command line.

### marimo fixture

`fx/marimo/marimo.qmd`, in a project that holds the marimo engine package, copied from
`crates/quarto-core/tests/fixtures/extensions/marimo/_extensions/marimo` into `fx/marimo/_extensions/`. `uv` must be on
`PATH`.

````text
---
title: marimo record
execute:
  record: true
---

```{python .marimo}
import marimo as mo
1 + 1
```

```{python .marimo}
mo.md("**MARIMO-MD**")
```
````

Run `q2 render marimo.qmd`; the record is `.quarto/execution-records/marimo.ipynb`.

### Helper extension

The harness takes one `--extensionDevelopmentPath`. The fork\'s commit hook rejects new `.js` files, so the helper
is not committed there. Save these two files in a directory and point `POSITRON_E2E_ASSISTANT_PATH` at it.

`package.json`:

````json
{
	"name": "q2-raw-record-spike",
	"displayName": "q2 raw record spike",
	"version": "0.0.1",
	"publisher": "vscode",
	"license": "MIT",
	"engines": { "vscode": "^1.0.0" },
	"main": "./extension.js",
	"activationEvents": ["onCommand:q2spike.pushRaw"],
	"contributes": {
		"commands": [{ "command": "q2spike.pushRaw", "title": "q2 spike: push raw record", "category": "Quarto (q2)" }],
		"configuration": { "title": "q2spike", "properties": { "q2spike.recordPath": { "type": "string", "default": "" } } }
	}
}
````

`extension.js`:

````js
const vscode = require('vscode');
const fs = require('fs');

exports.activate = ctx => {
	ctx.subscriptions.push(vscode.commands.registerCommand('q2spike.pushRaw', async () => {
		const p = vscode.workspace.getConfiguration().get('q2spike.recordPath');
		const r = await vscode.commands.executeCommand('_positronQuarto.pushOutputsFromFile', p);
		fs.writeFileSync(p + '.result.json', JSON.stringify(r));
	}));
};
````

### Run

````bash
export POSITRON_E2E_ASSISTANT_PATH=<helper directory>
export Q2_RAW_FX=<directory holding knitr/ and marimo/ as above>
export Q2_RAW_OUT=<empty directory>
npx playwright test test/e2e/tests/quarto/q2-raw-record-experiment.test.ts --project e2e-electron
````

The test writes `<name>.json` (push result plus the scanned outputs) into `Q2_RAW_OUT`. It needs the Python and R
versions set as for the other Quarto e2e tests.

## Limits

- Julia, Python and structured knitr shapes are not covered here; the Phase A spike covered them.
- One fixture per shape. The knitr fallback causes tried were a PDF figure device and a replaced hook.
- The untracked fork file `q2-raw-record-spike.test.ts` (the Phase A spike, with hardcoded scratch paths) was left as it was.
