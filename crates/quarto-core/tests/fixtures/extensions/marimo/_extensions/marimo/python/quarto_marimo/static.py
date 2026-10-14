"""Project compiled marimo output into Quarto's static output records."""

from __future__ import annotations

import json
from typing import Any

from quarto_marimo.authoring import as_bool
from quarto_marimo.protocol import (
    CompiledMarimoCell,
    CompiledMarimoPage,
    MarimoCellRequest,
    MarimoPageRequest,
)


def render_static_page(
    request: MarimoPageRequest,
    page: CompiledMarimoPage,
) -> list[dict[str, Any]]:
    request_indices = [cell.index for cell in request.cells]
    compiled_indices = [cell.index for cell in page.cells]
    if request_indices != compiled_indices:
        raise RuntimeError(
            "marimo compiler returned cells that do not match the source page"
        )
    return [
        render_static_cell(request_cell, compiled_cell)
        for request_cell, compiled_cell in zip(
            request.cells,
            page.cells,
            strict=True,
        )
    ]


def render_static_cell(
    request: MarimoCellRequest,
    cell: CompiledMarimoCell,
) -> dict[str, Any]:
    render = option_section(cell.options, "render")
    execution = option_section(cell.options, "execution")
    include = as_bool(render.get("include"), True)
    show_output = (
        include
        and as_bool(execution.get("enabled"), True)
        and as_bool(render.get("output"), True)
    )
    base = {
        "displayCode": include and as_bool(render.get("source")),
        "code": request.source,
        "language": str(cell.options.get("language") or "python"),
    }
    output = cell.output
    if not show_output or output is None:
        return {"type": "html", "value": "", **base}

    mimetype = output.mimetype
    if mimetype == "image/svg+xml":
        return {"type": "html", "value": str(output.data), **base}
    if mimetype.startswith("image/"):
        return {
            "type": "figure",
            "value": image_source(mimetype, output.data),
            **base,
        }
    if mimetype == "application/vnd.marimo+mimebundle":
        rendered = render_mimebundle(output.data, base)
        if rendered is not None:
            return rendered
    if mimetype == "text/plain":
        return {"type": "plain", "value": str(output.data), **base}
    if mimetype == "text/markdown":
        return {"type": "para", "value": str(output.data), **base}
    if mimetype == "text/html":
        return {"type": "html", "value": str(output.data), **base}
    if mimetype in {
        "application/vnd.marimo+error",
        "application/vnd.marimo+traceback",
    }:
        return {"type": "blockquote", "value": error_message(output.data), **base}
    return {"type": "html", "value": output.html, **base}


def render_mimebundle(
    value: Any,
    base: dict[str, Any],
) -> dict[str, Any] | None:
    try:
        bundle = json.loads(value) if isinstance(value, str) else value
    except json.JSONDecodeError:
        return None
    if not isinstance(bundle, dict):
        return None
    if "image/svg+xml" in bundle:
        return {"type": "html", "value": str(bundle["image/svg+xml"]), **base}
    for mimetype in ("image/png", "image/jpeg"):
        if mimetype in bundle:
            return {
                "type": "figure",
                "value": image_source(mimetype, bundle[mimetype]),
                **base,
            }
    if "text/html" in bundle:
        return {"type": "html", "value": str(bundle["text/html"]), **base}
    if "text/plain" in bundle:
        return {"type": "plain", "value": str(bundle["text/plain"]), **base}
    return None


def image_source(mimetype: str, value: Any) -> str:
    source = str(value)
    if source.startswith(("data:", "http://", "https://", "./", "../")):
        return source
    return f"data:{mimetype};base64,{source}"


def error_message(value: Any) -> str:
    if isinstance(value, list):
        messages = [format_error(item) for item in value if isinstance(item, dict)]
        if messages:
            return "\n".join(messages)
    if isinstance(value, dict):
        return format_error(value)
    return str(value)


def format_error(error: dict[str, Any]) -> str:
    error_type = error.get("type")
    if error_type == "cycle":
        return "This cell is in a cycle"
    if error_type == "multiple-defs":
        return f"The variable '{error.get('name', '')}' was defined by another cell"
    if error_type == "setup-refs":
        return "The setup cell cannot have references"
    message = error.get("msg")
    if message:
        kind = error.get("exception_type") or error.get("error_type")
        return f"{kind}: {message}" if kind else str(message)
    return str(error_type or "marimo error")


def option_section(options: dict[str, Any], key: str) -> dict[str, Any]:
    value = options.get(key)
    return value if isinstance(value, dict) else {}
