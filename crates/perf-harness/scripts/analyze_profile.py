#!/usr/bin/env python3
"""Offline self-time analyzer for samply profiles.

Emits a top-N ranked self-time table from a samply-recorded profile
(`.json.gz`), resolving raw addresses through the sidecar symbol table
produced by `samply record --unstable-presymbolicate`.

Why this script exists
----------------------

samply has no built-in CLI analyzer — its normal workflow is to launch
a web UI. This script extracts the same "top self-time symbols" view
offline so it can be diffed across fixtures, checked into commit
messages, or compared before/after a fix without context-switching to
a browser.

Profile format (Firefox Profiler columnar JSON)
-----------------------------------------------

Each thread has parallel arrays:

* `samples.stack[k]`   → stackTable index for sample k
* `stackTable.frame[i]` → frameTable index at stack position i
* `frameTable.func[j]`  → funcTable index for frame j
* `funcTable.name[m]`   → stringArray index for func m
* `stringArray[n]`       → the actual string

Resolving an address
--------------------

With `--unstable-presymbolicate`, samply writes a sidecar
`<profile>.syms.json` of the form::

    { "string_table": [...], "data": [ { "debug_name": "...",
        "symbol_table": [ {"rva": N, "size": N, "symbol": idx}, ... ]
      }, ... ] }

The profile's stringArray contains entries like ``"0x262b5c"`` (a
runtime-virtual address) alongside a module name reachable via
``funcTable.resource → resourceTable.name``. For each such address,
binary-search the matching module's symbol_table (sorted by rva) for
the largest rva ≤ address with rva + size > address, and substitute
the resolved symbol name.

The sidecar is optional; if missing, raw addresses are reported.

Usage
-----

::

    analyze_profile.py <profile.json.gz> [--top N] [--syms PATH]

Default ``--syms`` is ``<profile>.syms.json`` (i.e. strip the trailing
``.gz`` from the profile path and append ``.syms.json``), matching
samply's default naming.
"""
import argparse
import gzip
import json
import sys
from bisect import bisect_right
from collections import Counter
from pathlib import Path


def load_syms(syms_path: Path) -> dict[str, tuple[list[int], list[int], list[str]]]:
    """Parse a samply syms.json sidecar into {module_name: (rvas, sizes, symbols)}.

    Entries are sorted by rva so callers can `bisect_right` them.
    """
    with open(syms_path) as f:
        data = json.load(f)
    strings = data["string_table"]
    modules: dict[str, tuple[list[int], list[int], list[str]]] = {}
    for entry in data["data"]:
        name = entry["debug_name"]
        sym_tab = sorted(entry.get("symbol_table", []), key=lambda x: x["rva"])
        if not sym_tab:
            continue
        rvas = [s["rva"] for s in sym_tab]
        sizes = [s["size"] for s in sym_tab]
        syms = [strings[s["symbol"]] for s in sym_tab]
        modules[name] = (rvas, sizes, syms)
    return modules


def resolve_address(
    modules: dict[str, tuple[list[int], list[int], list[str]]],
    module_name: str,
    addr_hex: str,
) -> tuple[str, bool]:
    """Return (resolved_symbol_or_original, was_resolved)."""
    if module_name not in modules:
        return addr_hex, False
    try:
        addr = int(addr_hex, 16)
    except ValueError:
        return addr_hex, False
    rvas, sizes, syms = modules[module_name]
    i = bisect_right(rvas, addr) - 1
    if i < 0:
        return addr_hex, False
    if addr < rvas[i] + sizes[i]:
        return syms[i], True
    return addr_hex, False


def tally_self_time(
    profile: dict,
    modules: dict[str, tuple[list[int], list[int], list[str]]],
) -> Counter[tuple[str, str]]:
    """Walk all threads, tallying (resolved_symbol, module) per sample's top frame."""
    tally: Counter[tuple[str, str]] = Counter()
    for thread in profile.get("threads", []):
        strings = thread["stringArray"]
        stack_frame = thread["stackTable"]["frame"]
        frame_func = thread["frameTable"]["func"]
        func_name = thread["funcTable"]["name"]
        func_resource = thread["funcTable"].get("resource")
        resource_name = thread.get("resourceTable", {}).get("name", [])

        for s in thread["samples"].get("stack", []):
            if s is None:
                continue
            frame_idx = stack_frame[s]
            func_idx = frame_func[frame_idx]
            raw = strings[func_name[func_idx]]

            module = ""
            if func_resource is not None:
                r = func_resource[func_idx]
                if r is not None and 0 <= r < len(resource_name):
                    module = strings[resource_name[r]]

            resolved = raw
            if raw.startswith("0x") and module in modules:
                sym, ok = resolve_address(modules, module, raw)
                if ok:
                    resolved = sym

            tally[(resolved, module)] += 1
    return tally


def print_report(profile_path: Path, tally: Counter[tuple[str, str]], top_n: int) -> None:
    total = sum(tally.values())
    if total == 0:
        print("No samples found", file=sys.stderr)
        sys.exit(1)

    print(f"profile: {profile_path}")
    print(f"total_samples: {total}")
    print(f"top {top_n} by self-time:")
    print(f"{'pct':>6}  {'samples':>8}  symbol")
    print(f"{'-' * 6}  {'-' * 8}  {'-' * 80}")
    for (sym, module), count in tally.most_common(top_n):
        pct = 100.0 * count / total
        label = sym if len(sym) <= 150 else sym[:147] + "..."
        module_short = f"  [{module}]" if module else ""
        print(f"{pct:5.2f}%  {count:8d}  {label}{module_short}")


def default_syms_path(profile_path: Path) -> Path:
    """Match samply's sidecar naming: <profile>.syms.json (strip trailing .gz)."""
    name = profile_path.name
    if name.endswith(".json.gz"):
        stem = name[: -len(".gz")]
    else:
        stem = profile_path.stem
    return profile_path.with_name(stem + ".syms.json")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.strip().split("\n")[0])
    parser.add_argument(
        "profile",
        type=Path,
        help="samply profile file (.json.gz), as produced by `samply record -o ...`",
    )
    parser.add_argument(
        "--top",
        type=int,
        default=30,
        help="Number of top self-time symbols to report (default: 30)",
    )
    parser.add_argument(
        "--syms",
        type=Path,
        default=None,
        help="Path to syms sidecar (default: <profile>.syms.json alongside the profile)",
    )
    args = parser.parse_args()

    syms_path = args.syms or default_syms_path(args.profile)
    if syms_path.exists():
        modules = load_syms(syms_path)
    else:
        print(
            f"warning: no syms sidecar at {syms_path}; addresses will not be resolved. "
            f"Re-record with `samply record --unstable-presymbolicate` to get symbols.",
            file=sys.stderr,
        )
        modules = {}

    with gzip.open(args.profile, "rt") as f:
        profile = json.load(f)

    tally = tally_self_time(profile, modules)
    print_report(args.profile, tally, args.top)
    return 0


if __name__ == "__main__":
    sys.exit(main())
