# /// script
# requires-python = ">=3.10"
# dependencies = ["nbclient", "nbformat", "ipykernel", "matplotlib", "pandas", "numpy"]
# ///
"""bd-sr73 size measurement (EXR-4 Phase 0): today's engine capture vs an ipynb.

  uv run scripts/exr-record/size_measure.py DOC.qmd TRACE.json.gz FIGURES_DIR

DOC.qmd       the fixture (crates/quarto-core/tests/fixtures/records/size-measurement.qmd)
TRACE.json.gz `.quarto/trace/<stem>/latest.json.gz` from `q2 render` of DOC with `trace: true`
FIGURES_DIR   the engine's own supporting files, doc-relative root is its parent's parent:
              `<render dir>/doc_files/figure-html` (NOT all of `doc_files/`, which also holds
              the render's CSS/JS assets that the capture never contains)

Rebuilds `EngineCapture` as `quarto-trace/src/lib.rs:187` defines it (files as
`{path, contents_base64}`, sorted, as `collect_capture_files` emits) and gzips the array with
zlib level 6 like `gzip_captures`. The ipynb is the same cells run through a real kernel.
"""
import base64
import gzip
import json
import re
import sys
from pathlib import Path

import nbformat
from nbclient import NotebookClient

doc, trace_gz, figures = (Path(a) for a in sys.argv[1:4])
doc_dir = figures.parent.parent  # the render dir: `doc_files/figure-html` -> `<render dir>`

# --- today's capture -------------------------------------------------------------------
trace = json.loads(gzip.open(trace_gz).read())
cap = trace["engine_captures"][0]
files = [
    {
        "path": p.relative_to(doc_dir).as_posix(),
        "contents_base64": base64.b64encode(p.read_bytes()).decode(),
    }
    for p in sorted(figures.rglob("*"))
    if p.is_file()
]
capture = {"engine_name": cap["engine_name"], "input_qmd": cap["input_qmd"], "result": cap["result"]}
if files:
    capture["files"] = files
capture_json = json.dumps([capture], separators=(",", ":")).encode()
capture_gz = gzip.compress(capture_json, compresslevel=6)

# --- the same cells as an ipynb, run in a real kernel ---------------------------------------
cells = re.findall(r"```\{python\}\n(.*?)```", doc.read_text(), flags=re.S)
nb = nbformat.v4.new_notebook(cells=[nbformat.v4.new_code_cell(c.rstrip("\n")) for c in cells])
NotebookClient(nb, kernel_name="python3", timeout=120).execute()
ipynb = (nbformat.writes(nb, version=4) + "\n").encode()
ipynb_gz = gzip.compress(ipynb, compresslevel=6)

png_bytes = sum(len(base64.b64decode(f["contents_base64"])) for f in files)
png_b64 = sum(len(f["contents_base64"]) for f in files)
md_len = len(cap["result"]["markdown"])
print("| item | bytes |")
print("|---|---:|")
print(f"| figure files in capture (raw PNG) | {png_bytes} |")
print(f"| figure files in capture (base64) | {png_b64} |")
print(f"| capture `result.markdown` | {md_len} |")
print(f"| capture `input_qmd` | {len(cap['input_qmd'])} |")
print(f"| capture JSON (array, minified) | {len(capture_json)} |")
print(f"| capture gzipped | {len(capture_gz)} |")
print(f"| ipynb with inline base64 | {len(ipynb)} |")
print(f"| ipynb gzipped | {len(ipynb_gz)} |")
print(f"| ratio ipynb.gz / capture.gz | {len(ipynb_gz) / len(capture_gz):.2f} |")
