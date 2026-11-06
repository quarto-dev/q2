# /// script
# requires-python = ">=3.10"
# dependencies = ["matplotlib", "numpy"]
# ///
"""bd-sr73, the knitr case (EXR-4 Phase 0): a HAND-BUILT record, sized like a typical knitr document.

  uv run scripts/exr-record/size_knitr_handbuilt.py

knitr records cannot be produced before EXR-11. This builds the shape the plan describes: one
rendered-markdown `display_data` cell (`text/x-quarto-markdown`) per chunk, figures held in the
notebook-level `quarto_files` map as `{data, size}` (base64), and compares its gzipped size with
a capture of the same content (`EngineCapture` with `files`). Three DIFFERENT figures are
generated: identical copies would compress unrealistically well. Re-measure when EXR-11 lands.
"""
import base64
import gzip
import io
import json

import matplotlib

matplotlib.use("agg")
import matplotlib.pyplot as plt
import numpy as np

rng = np.random.default_rng(0)
pngs = {}
for i in range(3):
    fig, ax = plt.subplots(figsize=(7, 5), dpi=96)
    if i == 0:
        ax.scatter(rng.normal(size=300), rng.normal(size=300), s=8)
    elif i == 1:
        ax.hist(rng.normal(size=2000), bins=40)
    else:
        ax.plot(np.cumsum(rng.normal(size=400)))
    buf = io.BytesIO()
    fig.savefig(buf, format="png")
    plt.close(fig)
    pngs[f"report_files/figure-html/unnamed-chunk-{i + 1}-1.png"] = buf.getvalue()

# Rendered markdown of a typical knitr document: ~8 chunks of code, output and prose.
chunk_md = []
for i in range(8):
    fig = f"\n![](report_files/figure-html/unnamed-chunk-{i + 1}-1.png)\n" if i < 3 else ""
    chunk_md.append(
        f"::: {{.cell}}\n```{{.r .cell-code}}\nsummary(lm(y ~ x{i}, data = d))\n```\n\n"
        f"::: {{.cell-output .cell-output-stdout}}\n```\nCall:\nlm(formula = y ~ x{i}, data = d)\n"
        + "Residuals:\n   Min     1Q Median     3Q    Max\n-2.100 -0.600  0.010  0.590  2.900\n" * 3
        + f"```\n:::\n{fig}:::\n"
    )

files = {p: {"data": base64.b64encode(b).decode(), "size": len(b)} for p, b in pngs.items()}
cells = [
    {
        "cell_type": "code",
        "execution_count": None,
        "id": f"{i:016x}",
        "metadata": {"quarto_cell_id": f"{i}-{i:08x}-unlabeled", "quarto_engine": "knitr"},
        "outputs": [
            {
                "output_type": "display_data",
                "data": {"text/x-quarto-markdown": md},
                "metadata": {"quarto_format": "html"},
            }
        ],
        "source": [f"summary(lm(y ~ x{i}, data = d))"],
    }
    for i, md in enumerate(chunk_md)
]
record = {
    "cells": cells,
    "metadata": {"quarto_record_version": 1, "quarto_cache_version": 1, "quarto_files": files},
    "nbformat": 4,
    "nbformat_minor": 5,
}
rec = (json.dumps(record, indent=1, sort_keys=True) + "\n").encode()
cap = json.dumps(
    [
        {
            "engine_name": "knitr",
            "input_qmd": "x" * 5300,
            "result": {"markdown": "".join(chunk_md)},
            "files": [{"path": p, "contents_base64": f["data"]} for p, f in files.items()],
        }
    ],
    separators=(",", ":"),
).encode()
png_total = sum(len(b) for b in pngs.values())
print("| item | bytes |")
print("|---|---:|")
print(f"| figure files (raw PNG, 3 figures) | {png_total} |")
print(f"| rendered markdown, 8 chunks | {len(''.join(chunk_md))} |")
print(f"| hand-built record | {len(rec)} |")
print(f"| hand-built record gzipped | {len(gzip.compress(rec, 6))} |")
print(f"| comparable capture gzipped | {len(gzip.compress(cap, 6))} |")
