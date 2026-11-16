"""Collect one Quarto document into one marimo page request."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import TYPE_CHECKING

from marimo._convert.markdown.to_ir import MARIMO_MD

from quarto_marimo.authoring import (
    cell_options_patch,
    execution_options_patch,
    extract_cell_config,
    page_options_from_root,
)
from quarto_marimo.protocol import (
    PAGE_PROTOCOL_VERSION,
    MarimoCellRequest,
    MarimoPageMetadata,
    MarimoPageRequest,
)

if TYPE_CHECKING:
    from xml.etree.ElementTree import Element


def collect_page(
    root: Element,
    *,
    filename: str,
    global_eval: bool,
) -> MarimoPageRequest:
    global_options = page_options_from_root(root)
    if not global_eval:
        global_options["eval"] = False
    defaults = execution_options_patch(global_options)

    cells: list[MarimoCellRequest] = []
    for child in root:
        if child.tag == MARIMO_MD:
            continue
        local_options, source = extract_cell_config(str(child.text or ""))
        options = cell_options_patch(local_options, dict(child.attrib))
        if not global_eval:
            options.pop("execution", None)
        cells.append(
            MarimoCellRequest(
                index=len(cells),
                source=source,
                options=options,
            )
        )

    metadata = MarimoPageMetadata(
        pyproject=str(global_options.get("pyproject") or ""),
        setup_cells=setup_cell_requests(str(global_options.get("header") or "")),
    )
    identity = page_identity(
        metadata=metadata,
        defaults=defaults,
        cells=tuple(cells),
    )
    digest = identity.rsplit(":", 1)[-1]
    if filename:
        source_path = Path(filename)
        if source_path.is_absolute():
            try:
                source_name = str(source_path.relative_to(Path.cwd()))
            except ValueError:
                source_name = source_path.name
        else:
            source_name = str(source_path)
    else:
        source_name = f"quarto-marimo-{digest}.qmd"
    return MarimoPageRequest(
        identity=identity,
        filename=source_name,
        metadata=metadata,
        defaults=defaults,
        cells=tuple(cells),
    )


def page_identity(
    *,
    metadata: MarimoPageMetadata,
    defaults: dict[str, object],
    cells: tuple[MarimoCellRequest, ...],
) -> str:
    source = {
        "protocolVersion": PAGE_PROTOCOL_VERSION,
        "metadata": {
            "pyproject": metadata.pyproject,
            "setupCells": [
                cell.to_json(include_position=False) for cell in metadata.setup_cells
            ],
        },
        "defaults": defaults,
        "cells": [cell.to_json(include_position=False) for cell in cells],
    }
    encoded = json.dumps(
        source,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    )
    digest = hashlib.sha256(encoded.encode("utf-8")).hexdigest()[:16]
    return f"quarto-marimo:{digest}"


def setup_cell_requests(header: str) -> tuple[MarimoCellRequest, ...]:
    if not header.strip():
        return ()
    return (
        MarimoCellRequest(
            index=0,
            source=header,
            options={"language": "python"},
        ),
    )
