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

from marimo._cli.sandbox import construct_uv_flags
from marimo._utils.inline_script_metadata import PyProjectReader


def extract_command(header: str) -> list[str]:
    """Build the ``uv run`` argument list for one Quarto document.

    Quarto passes dependency declarations as raw YAML text. We first wrap that
    in inline script metadata so dependency resolution follows marimo's
    existing sandbox rules instead of a second copy of the same logic here.
    """
    header = dedent(header).strip()
    if not header.splitlines() or header.splitlines()[0].strip() != "# /// script":
        header = "\n# ".join(["# /// script", *header.splitlines(), "///"])
    pyproject = PyProjectReader.from_script(header)
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", delete=False, suffix=".txt"
    ) as temp_file:
        flags = construct_uv_flags(pyproject, temp_file, [], [])
        temp_file.flush()
    return ["run"] + flags  # type: ignore[no-any-return]


if __name__ == "__main__":
    assert len(sys.argv) == 1, f"Unexpected call format got {sys.argv}"

    # The engine always sends UTF-8 bytes; do not trust the host locale codec.
    header = sys.stdin.buffer.read().decode("utf-8")

    command = extract_command(header)
    sys.stdout.buffer.write(json.dumps(command).encode("utf-8"))
    sys.stdout.flush()
