#!/usr/bin/env python3
"""Replay a captured Quarto 1 pandoc invocation standalone (no Q1 TypeScript).

This is the stand-in for a q2 orchestrator: it reconstructs the defaults file,
metadata file, filter-params env var, and dependency file, then invokes pandoc
directly. Usage:

  replay.py <capture-call-dir> <workdir> [--pandoc BIN] [--env KEY=VAL ...]
"""
import argparse
import base64
import glob
import json
import os
import shutil
import subprocess
import sys

ap = argparse.ArgumentParser()
ap.add_argument("capture")
ap.add_argument("workdir")
ap.add_argument("--pandoc", default="/Users/gordon/src/q2/external-sources/quarto-cli/package/dist/bin/tools/aarch64/pandoc")
ap.add_argument("--env", action="append", default=[], help="extra KEY=VAL env")
ap.add_argument("--fixture", default=None, help="dir with resources (png, bib) to copy into workdir")
ap.add_argument("--partials", default=None, help="dir with template partials to stage next to the template")
ap.add_argument("--input-json", default=None,
                help="pampa-route: use this Pandoc JSON (from `pampa -t json`) as input "
                     "instead of the captured .md; sets `from: json` and injects the "
                     "quarto_pandoc_reader_opts meta stub")
ap.add_argument("--prepend-filter", action="append", default=[],
                help="Lua filter path(s) to prepend to the defaults filter list (e.g. q2-compat.lua)")
args = ap.parse_args()

cap = os.path.abspath(args.capture)
wd = os.path.abspath(args.workdir)
os.makedirs(wd, exist_ok=True)

def one(pattern):
    m = glob.glob(os.path.join(cap, "files", pattern))
    assert len(m) == 1, f"expected 1 match for {pattern}, got {m}"
    return m[0]

defaults_src = one("1-defaults-*.yml")
metadata_src = one("2-metadata-file-*.yml")
input_src = None if args.input_json else one("input-*.md")

# fixture resources
if args.fixture:
    for f in glob.glob(os.path.join(args.fixture, "*")):
        if not f.endswith(".qmd"):
            shutil.copy(f, wd)

# defaults: rewrite the temp template path to the captured copy, if any,
# staged into a dir that also holds the format's partials (pandoc resolves
# partials relative to the template's directory)
defaults = open(defaults_src).read()
tpl = glob.glob(os.path.join(cap, "files", "defaultsref-template.patched"))
if tpl:
    import re
    tpl_dir = os.path.join(wd, "template")
    if args.partials:
        shutil.copytree(args.partials, tpl_dir, dirs_exist_ok=True)
    else:
        os.makedirs(tpl_dir, exist_ok=True)
    tpl_dst = os.path.join(tpl_dir, "template.patched")
    shutil.copy(tpl[0], tpl_dst)
    defaults = re.sub(r"^template: .*$", f"template: {tpl_dst}", defaults, flags=re.M)
import re as _re
if args.input_json:
    defaults = _re.sub(r"^from: .*$", "from: json", defaults, flags=_re.M)
for f in reversed(args.prepend_filter):
    defaults = defaults.replace("filters:\n", f"filters:\n  - {os.path.abspath(f)}\n", 1)
defaults_path = os.path.join(wd, "defaults.yml")
open(defaults_path, "w").write(defaults)

shutil.copy(metadata_src, os.path.join(wd, "metadata.yml"))
if args.input_json:
    doc = json.load(open(args.input_json))
    # Q1's normalize/capturereaderstate.lua expects the qmd-reader to have
    # stashed reader options in meta; an empty MetaMap yields defaults.
    doc["meta"]["quarto_pandoc_reader_opts"] = {"t": "MetaMap", "c": {}}
    input_name = "input.json"
    json.dump(doc, open(os.path.join(wd, input_name), "w"))
else:
    input_name = "input.md"
    shutil.copy(input_src, os.path.join(wd, input_name))

# filter params: patch temp paths to fresh ones in workdir
params_file = os.path.join(cap, "filter-params.json")
params = json.load(open(params_file))
results_file = os.path.join(wd, "filter-results.json")
params["results-file"] = results_file
params["notebook-context"] = os.path.join(wd, "notebook-context.json")
params["quarto-source"] = input_name
params_b64 = base64.b64encode(json.dumps(params).encode()).decode()

dep_file = os.path.join(wd, "filter-deps.jsonl")
open(dep_file, "w").close()

# data-dir from argv
argv = open(os.path.join(cap, "argv.txt")).read().split("\n")
data_dir = argv[argv.index("--data-dir") + 1]

env = {
    "PATH": os.environ["PATH"],
    "HOME": os.environ["HOME"],
    "QUARTO_FILTER_PARAMS": params_b64,
    "QUARTO_FILTER_DEPENDENCY_FILE": dep_file,
}
for kv in args.env:
    k, _, v = kv.partition("=")
    env[k] = v

cmd = [args.pandoc, "--defaults", defaults_path, input_name,
       "--metadata-file", "metadata.yml", "--data-dir", data_dir]
print("+ " + " ".join(cmd), file=sys.stderr)
r = subprocess.run(cmd, cwd=wd, env=env)
sys.exit(r.returncode)
