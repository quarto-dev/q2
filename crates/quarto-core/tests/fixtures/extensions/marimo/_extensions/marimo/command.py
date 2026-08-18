#!/usr/bin/env python3
"""Convert document-level dependency metadata into ``uv run`` arguments.

The TypeScript engine owns process orchestration, so this helper stays focused
on one job: reuse marimo's sandbox logic to interpret Quarto's ``pyproject``
frontmatter and hand back the flags needed for the actual extraction step.
"""

from __future__ import annotations

import json
import sys
import tempfile
from textwrap import dedent
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from marimo._cli.sandbox import construct_uv_flags  # type: ignore[no-redef]
    from marimo._utils.inline_script_metadata import (
        PyProjectReader,  # type: ignore[no-redef]
    )
else:
    try:
        from marimo._internal.sandbox import PyProjectReader, construct_uv_flags
    except ImportError:
        from marimo._cli.sandbox import construct_uv_flags  # type: ignore[no-redef]
        from marimo._utils.inline_script_metadata import (
            PyProjectReader,  # type: ignore[no-redef]
        )


def extract_command(header: str) -> list[str]:
    """Build the ``uv run`` argument list for one Quarto document.

    Quarto passes dependency declarations as raw YAML text. We first wrap that
    in inline script metadata so dependency resolution follows marimo's
    existing sandbox rules instead of a second copy of the same logic here.
    """
    if not header.startswith("#"):
        header = "\n# ".join(["# /// script", *header.splitlines(), "///"])
    pyproject = PyProjectReader.from_script(header)
    with tempfile.NamedTemporaryFile(
        mode="w", delete=False, suffix=".txt"
    ) as temp_file:
        flags = construct_uv_flags(pyproject, temp_file, [], [])
        temp_file.flush()
    return ["run"] + flags  # type: ignore[no-any-return]


if __name__ == "__main__":
    assert len(sys.argv) == 1, f"Unexpected call format got {sys.argv}"

    header = dedent(sys.stdin.read())

    command = extract_command(header)
    sys.stdout.write(json.dumps(command))
