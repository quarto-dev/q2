#!/usr/bin/env python3
"""Scan workspace crates for compile-time file reads that corgi must be told about.

corgi (https://github.com/ConradIrwin/corgi) sandboxes rustc and build
scripts: only a package's own `.rs` files are readable unless the package
declares more under `[extra-inputs]` in corgi.toml. This script finds the
literal-path `include_str!` / `include_bytes!` / `include_dir!` (and the
`concat!(env!("CARGO_MANIFEST_DIR"), "...")` form) in each member crate and
prints a TOML `[extra-inputs]` table of package-root-relative globs.

It is a heuristic: computed paths (`env!("FOO_DIR")`, build-script reads)
are not found and live in the hand-written part of corgi.toml.

Usage: python3 -I scripts/corgi/scan-extra-inputs.py [workspace-root]

Writes <workspace-root>/corgi.toml: scripts/corgi/corgi.base.toml (the
hand-written tools/env/extra-inputs) with the scanned globs merged into its
[extra-inputs] table.
"""
import json
import os
import subprocess
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
MACRO = re.compile(
    # include_error_table! is the workspace's own proc macro
    # (crates/*/error-message-macros); it resolves against the crate root.
    r'include_(str|bytes|dir|error_table)!\s*\(\s*'
    r'(?:concat!\s*\(\s*env!\s*\(\s*"CARGO_MANIFEST_DIR"\s*\)\s*,\s*"(?P<concat>[^"]+)"\s*\)'
    r'|"(?P<lit>[^"]+)")',
    re.S,
)
# rust-embed: `#[folder = "dir/"]`, relative to the crate root.
FOLDER = re.compile(r'#\[folder\s*=\s*"([^"]+)"\s*\]')


def cargo_metadata():
    return json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout
    )


def insta_users():
    return sorted(
        p["name"]
        for p in cargo_metadata()["packages"]
        if any(d["name"] == "insta" for d in p["dependencies"])
    )


def crate_dirs():
    """Workspace members, per `cargo metadata` (globbed members plus nested
    path dependencies such as crates/quarto-math/vendor/*)."""
    meta = cargo_metadata()
    for pkg in sorted(meta["packages"], key=lambda p: p["name"]):
        yield pkg["name"], Path(pkg["manifest_path"]).parent


def glob_for(path: Path, crate: Path):
    rel = os.path.relpath(path, crate)
    rel = rel.replace(os.sep, "/")
    if path.is_dir():
        return rel.rstrip("/") + "/**"
    if path.suffix == ".rs" and not rel.startswith(".."):
        return None  # corgi already grants in-package .rs files
    return rel


out = {}
for name, crate in crate_dirs():
    globs = set()
    for src in crate.rglob("*.rs"):
        if "target" in src.relative_to(crate).parts:
            continue
        text = src.read_text(errors="replace")
        for m in MACRO.finditer(text):
            kind = m.group(1)
            if m.group("concat"):
                target = crate / m.group("concat").lstrip("/")
            else:
                lit = m.group("lit")
                if lit.startswith("$CARGO_MANIFEST_DIR"):
                    target = crate / lit[len("$CARGO_MANIFEST_DIR"):].lstrip("/")
                elif lit.startswith("$"):
                    continue  # computed by a build script; hand-written
                elif kind in ("dir", "error_table"):
                    target = crate / lit
                else:
                    target = src.parent / lit
            target = Path(os.path.normpath(target))
            if not target.exists():
                print(f"# warning: {name}: {src.relative_to(ROOT)} -> missing {target}", file=sys.stderr)
                continue
            g = glob_for(target, crate)
            if g:
                globs.add(g)
        for m in FOLDER.finditer(text):
            target = Path(os.path.normpath(crate / m.group(1)))
            if target.is_dir():
                globs.add(glob_for(target, crate))
    # Test data read at *runtime*. corgi keys a cached test pass on the
    # harness binary, i.e. on its compile inputs, so a fixture or snapshot
    # edit only re-runs the test if the file is declared here. Heuristic:
    # the package's own test-data directories. It does not cover tests
    # that read other packages' files, or external tools (pandoc, node, …);
    # use `corgi test --force` when in doubt.
    for d in ("tests", "snapshots", "src/snapshots", "test-data", "test-fixtures", "fixtures"):
        if (crate / d).is_dir() and any(
            f.is_file() and f.suffix != ".rs" for f in (crate / d).rglob("*")
        ):
            globs.add(f"{d}/**")
    if globs:
        out[name] = sorted(globs)

base_path = ROOT / "scripts" / "corgi" / "corgi.base.toml"
base_text = base_path.read_text()
manual = tomllib.loads(base_text).get("extra-inputs", {})
for name, globs in manual.items():
    out[name] = sorted(set(out.get(name, [])) | set(globs))

# Keep the base file verbatim up to its [extra-inputs] table, then emit
# the merged table (the base file keeps [extra-inputs] last).
head = base_text.split("\n[extra-inputs]\n")[0].rstrip() + "\n"
users = ", ".join(f'"{n}"' for n in insta_users())
head = head.replace("packages = []  # @insta-users", f"packages = [{users}]")
lines = [
    "# GENERATED by scripts/corgi/scan-extra-inputs.py from",
    "# scripts/corgi/corgi.base.toml; edit those, not this file.",
    "",
    head,
    "[extra-inputs]",
]
for name in sorted(out):
    items = "".join(f'    "{g}",\n' for g in out[name])
    lines.append(f'{name} = [\n{items}]')
(ROOT / "corgi.toml").write_text("\n".join(lines) + "\n")
print(f"wrote {ROOT / 'corgi.toml'} ({len(out)} packages)")
