"""Subprocess entry point for the Quarto marimo engine."""

from __future__ import annotations

import asyncio
import json
import os
import sys
from collections.abc import Callable
from contextlib import redirect_stdout
from pathlib import Path
from typing import Any, ClassVar, cast
from xml.etree.ElementTree import Element

from marimo._convert.markdown.to_ir import (
    MarimoMdParser,
    SafeWrap as SafeWrapGeneric,
)

from quarto_marimo.authoring import normalize_markdown
from quarto_marimo.compiler import compile_page
from quarto_marimo.document import collect_page
from quarto_marimo.protocol import CompiledMarimoPage
from quarto_marimo.static import render_static_page

ConversionResult = dict[str, Any]
SafeWrap = SafeWrapGeneric[ConversionResult]
ExportCallback = Callable[[Element], SafeWrap]


def convert_markdown(
    text: str,
    *,
    filename: str,
    interactive: bool,
    global_eval: bool = True,
    bare_sql: bool = False,
) -> dict[str, Any]:
    callback = (
        interactive_export(filename=filename, global_eval=global_eval)
        if interactive
        else static_export(filename=filename, global_eval=global_eval)
    )

    class QuartoMarimoParser(MarimoMdParser):
        output_formats: ClassVar[dict[str, ExportCallback]] = {  # type: ignore[assignment, misc]
            "quarto-marimo": callback,
        }

    parser = QuartoMarimoParser(output_format="quarto-marimo")  # type: ignore[arg-type]
    return cast(ConversionResult, parser.convert(normalize_markdown(text, bare_sql)))


def interactive_export(
    *,
    filename: str,
    global_eval: bool,
) -> ExportCallback:
    def export(root: Element) -> SafeWrap:
        request = collect_page(
            root,
            filename=filename,
            global_eval=global_eval,
        )
        page = asyncio.run(
            compile_page(
                request.to_json(),
                development_url=os.environ.get("QUARTO_MARIMO_DEBUG_ENDPOINT"),
                version_override=os.environ.get("QUARTO_MARIMO_VERSION"),
            )
        )
        return SafeWrap(
            {
                "kind": "page",
                "page": page,
            }
        )

    return export


def static_export(
    *,
    filename: str,
    global_eval: bool,
) -> ExportCallback:
    def export(root: Element) -> SafeWrap:
        request = collect_page(
            root,
            filename=filename,
            global_eval=global_eval,
        )
        page = CompiledMarimoPage.from_json(
            asyncio.run(compile_page(request.to_json()))
        )
        return SafeWrap(
            {
                "kind": "static",
                "outputs": render_static_page(request, page),
            }
        )

    return export


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if len(args) not in (2, 3, 4):
        raise ValueError(
            "expected reference file, output mode, and optional eval and bare-sql modes"
        )
    reference_file, output_mode = args[:2]
    global_eval = args[2].lower() == "yes" if len(args) >= 3 else True
    bare_sql = args[3].lower() == "yes" if len(args) >= 4 else False
    interactive = output_mode.lower() == "html"
    os.environ["MARIMO_NO_JS"] = str(not interactive).lower()

    # The engine always sends UTF-8 bytes. Bypass the locale-dependent text
    # wrappers so a cp932 or similar host locale cannot corrupt the document.
    source = sys.stdin.buffer.read().decode("utf-8")
    if not source:
        source = Path(reference_file).read_text(encoding="utf-8")
    with redirect_stdout(sys.stderr):
        result = convert_markdown(
            source,
            filename=reference_file,
            interactive=interactive,
            global_eval=global_eval,
            bare_sql=bare_sql,
        )
    sys.stdout.buffer.write(json.dumps(result).encode("utf-8"))
    sys.stdout.flush()
    return 0
