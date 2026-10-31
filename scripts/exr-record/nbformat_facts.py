# /// script
# requires-python = ">=3.10"
# dependencies = ["nbformat==5.11.1"]
# ///
"""nbformat validator facts the EXR-4 record format relies on (EXR-4 Phase 0).

Run:  uv run scripts/exr-record/nbformat_facts.py [record.ipynb ...]

With no arguments, checks every fact below and the checked-in sample record.
With arguments, validates each file and exits non-zero on any error or warning
(the golden-record gate used by EXR-4 Phase 4).
"""
import copy
import sys
import warnings
from pathlib import Path

import nbformat
from nbformat.validator import normalize, validate
from nbformat import ValidationError

SAMPLE = (
    Path(__file__).resolve().parents[2]
    / "crates/quarto-core/tests/fixtures/records/sample.ipynb"
)


def nb(minor=5, cells=None):
    return {"nbformat": 4, "nbformat_minor": minor, "metadata": {}, "cells": cells or []}


def code_cell(cell_id="aaaa", outputs=None, **extra):
    cell = {
        "cell_type": "code",
        "execution_count": None,
        "metadata": {},
        "source": ["x <- 1"],
        "outputs": outputs or [],
    }
    if cell_id is not None:
        cell["id"] = cell_id
    cell.update(extra)
    return cell


def check(notebook):
    """Return (valid, [warning category names])."""
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        try:
            validate(notebook)
            valid = True
        except ValidationError:
            valid = False
    return valid, sorted({w.category.__name__ for w in caught})


FACTS = []


def fact(fn):
    FACTS.append(fn)
    return fn


@fact
def minor5_cell_with_id_validates_cleanly():
    assert check(nb(cells=[code_cell("abc")])) == (True, [])


@fact
def minor5_cell_without_id_warns():
    assert check(nb(cells=[code_cell(None)])) == (True, ["MissingIDFieldWarning"])


@fact
def minor4_notebook_with_id_fails():
    valid, _ = check(nb(minor=4, cells=[code_cell("abc")]))
    assert not valid


@fact
def id_length_64_valid_65_fails():
    assert check(nb(cells=[code_cell("a" * 64)])) == (True, [])
    valid, _ = check(nb(cells=[code_cell("a" * 65)]))
    assert not valid


@fact
def hash_with_dedup_suffix_is_a_valid_id():
    assert check(nb(cells=[code_cell("4390f67858f3f1c7-2")])) == (True, [])


@fact
def duplicate_ids_warn_and_are_renamed():
    notebook = nb(cells=[code_cell("dup"), code_cell("dup")])
    assert check(copy.deepcopy(notebook)) == (True, ["DuplicateCellId"])
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        _, normalized = normalize(copy.deepcopy(notebook))
    ids = [c["id"] for c in normalized["cells"]]
    assert len(set(ids)) == 2, ids


@fact
def quarto_mime_and_metadata_validate():
    out = {
        "output_type": "display_data",
        "data": {"text/x-quarto-markdown": "hello"},
        "metadata": {"quarto_format": "html"},
    }
    cell = code_cell("c1", outputs=[out])
    cell["metadata"] = {
        "quarto_cell_id": "0-4390f678-unlabeled",
        "quarto_exec_hash": "0" * 64,
        "quarto_kind": "cell",
        "fig-width": 7,
    }
    notebook = nb(cells=[cell])
    notebook["metadata"] = {"quarto_record_version": 1, "quarto_cache_version": 1}
    assert check(notebook) == (True, [])


@fact
def execute_result_without_execution_count_fails():
    out = {"output_type": "execute_result", "data": {"text/plain": "1"}, "metadata": {}}
    valid, _ = check(nb(cells=[code_cell("c1", outputs=[out])]))
    assert not valid
    out["execution_count"] = None
    assert check(nb(cells=[code_cell("c1", outputs=[out])])) == (True, [])


@fact
def sample_record_validates_with_no_warning():
    assert check(nbformat.reads(SAMPLE.read_text(), as_version=4)) == (True, [])


def main(paths):
    if paths:
        bad = 0
        for p in paths:
            valid, warns = check(nbformat.reads(Path(p).read_text(), as_version=4))
            print(f"{p}: valid={valid} warnings={warns}")
            bad += (not valid) or bool(warns)
        return 1 if bad else 0
    failed = 0
    for f in FACTS:
        try:
            f()
            print(f"ok   {f.__name__}")
        except Exception as e:  # noqa: BLE001
            failed += 1
            print(f"FAIL {f.__name__}: {type(e).__name__}: {e}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
