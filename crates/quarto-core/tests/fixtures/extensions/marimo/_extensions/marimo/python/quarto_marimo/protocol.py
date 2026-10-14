"""Define the Python side of the marimo page protocol."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

PAGE_PROTOCOL_VERSION = 2
JsonObject = dict[str, Any]


@dataclass(frozen=True)
class MarimoCellRequest:
    index: int
    source: str
    options: JsonObject = field(default_factory=dict)
    start_line: int | None = None
    end_line: int | None = None

    def to_json(self, *, include_position: bool = True) -> JsonObject:
        payload: JsonObject = {
            "index": self.index,
            "source": self.source,
            "options": self.options,
        }
        if include_position and self.start_line is not None:
            payload["startLine"] = self.start_line
        if include_position and self.end_line is not None:
            payload["endLine"] = self.end_line
        return payload

    @classmethod
    def from_json(cls, value: Any) -> MarimoCellRequest:
        payload = require_object(value, "marimo cell request")
        index = payload.get("index")
        source = payload.get("source")
        options = payload.get("options")
        if options is None:
            options = {}
        if not isinstance(index, int) or isinstance(index, bool):
            raise TypeError("marimo cell index must be an integer")
        if not isinstance(source, str):
            raise TypeError("marimo cell source must be a string")
        if not isinstance(options, dict):
            raise TypeError("marimo cell options must be an object")
        return cls(
            index=index,
            source=source,
            options=dict(options),
            start_line=optional_int(payload.get("startLine"), "marimo cell start line"),
            end_line=optional_int(payload.get("endLine"), "marimo cell end line"),
        )


@dataclass(frozen=True)
class MarimoPageMetadata:
    pyproject: str = ""
    setup_cells: tuple[MarimoCellRequest, ...] = ()

    def to_json(self) -> JsonObject:
        payload: JsonObject = {}
        if self.pyproject:
            payload["pyproject"] = self.pyproject
        if self.setup_cells:
            payload["setupCells"] = [cell.to_json() for cell in self.setup_cells]
        return payload

    @classmethod
    def from_json(cls, value: Any) -> MarimoPageMetadata:
        payload = require_object(value, "marimo page metadata")
        pyproject = payload.get("pyproject")
        setup_cells = payload.get("setupCells")
        if setup_cells is None:
            setup_cells = []
        if pyproject is not None and not isinstance(pyproject, str):
            raise TypeError("marimo page pyproject must be a string")
        if not isinstance(setup_cells, list):
            raise TypeError("marimo setup cells must be an array")
        return cls(
            pyproject=pyproject or "",
            setup_cells=tuple(
                MarimoCellRequest.from_json(cell) for cell in setup_cells
            ),
        )


@dataclass(frozen=True)
class MarimoPageRequest:
    identity: str
    metadata: MarimoPageMetadata
    cells: tuple[MarimoCellRequest, ...]
    defaults: JsonObject = field(default_factory=dict)
    filename: str = ""
    protocol_version: int = PAGE_PROTOCOL_VERSION

    def to_json(self) -> JsonObject:
        payload: JsonObject = {
            "protocolVersion": self.protocol_version,
            "identity": self.identity,
            "metadata": self.metadata.to_json(),
            "cells": [cell.to_json() for cell in self.cells],
        }
        if self.filename:
            payload["filename"] = self.filename
        if self.defaults:
            payload["defaults"] = self.defaults
        return payload

    @classmethod
    def from_json(cls, value: Any) -> MarimoPageRequest:
        payload = require_object(value, "marimo page request")
        version = payload.get("protocolVersion")
        if version != PAGE_PROTOCOL_VERSION:
            raise ValueError(f"unsupported marimo page protocol: {version}")
        identity = payload.get("identity")
        cells = payload.get("cells")
        defaults = payload.get("defaults")
        filename = payload.get("filename")
        if defaults is None:
            defaults = {}
        if filename is None:
            filename = ""
        if not isinstance(identity, str) or not identity:
            raise TypeError("marimo page identity must be a non-empty string")
        if not isinstance(cells, list):
            raise TypeError("marimo page cells must be an array")
        if not isinstance(defaults, dict):
            raise TypeError("marimo page defaults must be an object")
        if not isinstance(filename, str):
            raise TypeError("marimo page filename must be a string")
        return cls(
            identity=identity,
            filename=filename,
            metadata=MarimoPageMetadata.from_json(payload.get("metadata")),
            defaults=dict(defaults),
            cells=tuple(MarimoCellRequest.from_json(cell) for cell in cells),
        )


@dataclass(frozen=True)
class CompiledMarimoOutput:
    mimetype: str
    data: Any
    html: str

    def to_json(self) -> JsonObject:
        return {
            "mimetype": self.mimetype,
            "data": self.data,
            "html": self.html,
        }

    @classmethod
    def from_json(cls, value: Any) -> CompiledMarimoOutput:
        payload = require_object(value, "compiled marimo output")
        mimetype = payload.get("mimetype")
        html = payload.get("html")
        if not isinstance(mimetype, str):
            raise TypeError("compiled marimo output mimetype must be a string")
        if "data" not in payload:
            raise TypeError("compiled marimo output data is required")
        if not isinstance(html, str):
            raise TypeError("compiled marimo output HTML must be a string")
        return cls(mimetype=mimetype, data=payload["data"], html=html)


@dataclass(frozen=True)
class CompiledMarimoCell:
    index: int
    html: str
    options: JsonObject
    output: CompiledMarimoOutput | None
    diagnostics: tuple[JsonObject, ...] = ()

    def to_json(self) -> JsonObject:
        payload: JsonObject = {
            "index": self.index,
            "html": self.html,
            "options": self.options,
            "output": self.output.to_json() if self.output else None,
        }
        if self.diagnostics:
            payload["diagnostics"] = list(self.diagnostics)
        return payload

    @classmethod
    def from_json(cls, value: Any) -> CompiledMarimoCell:
        payload = require_object(value, "compiled marimo cell")
        index = payload.get("index")
        html = payload.get("html")
        options = payload.get("options")
        if "output" not in payload:
            raise TypeError("compiled marimo cell output is required")
        output = payload["output"]
        diagnostics = payload.get("diagnostics")
        if diagnostics is None:
            diagnostics = []
        if not isinstance(index, int) or isinstance(index, bool):
            raise TypeError("compiled marimo cell index must be an integer")
        if not isinstance(html, str):
            raise TypeError("compiled marimo cell HTML must be a string")
        if not isinstance(options, dict):
            raise TypeError("compiled marimo cell options must be an object")
        if not isinstance(diagnostics, list) or not all(
            isinstance(item, dict) for item in diagnostics
        ):
            raise TypeError("compiled marimo cell diagnostics must be an array")
        return cls(
            index=index,
            html=html,
            options=dict(options),
            output=(
                CompiledMarimoOutput.from_json(output) if output is not None else None
            ),
            diagnostics=tuple(dict(item) for item in diagnostics),
        )


@dataclass(frozen=True)
class MarimoPageRuntime:
    id: str
    runtime_cell_count: int
    assets: JsonObject
    notebook_code: str = ""

    def to_json(self) -> JsonObject:
        payload: JsonObject = {
            "id": self.id,
            "runtimeCellCount": self.runtime_cell_count,
            "assets": self.assets,
        }
        if self.notebook_code:
            payload["notebookCode"] = self.notebook_code
        return payload

    @classmethod
    def from_json(cls, value: Any) -> MarimoPageRuntime:
        payload = require_object(value, "marimo page runtime")
        app_id = payload.get("id")
        runtime_cell_count = payload.get("runtimeCellCount")
        assets = payload.get("assets")
        notebook_code = payload.get("notebookCode")
        if notebook_code is None:
            notebook_code = ""
        if not isinstance(app_id, str) or not app_id:
            raise TypeError("marimo app id must be a non-empty string")
        if (
            not isinstance(runtime_cell_count, int)
            or isinstance(runtime_cell_count, bool)
            or runtime_cell_count < 0
        ):
            raise TypeError("marimo runtime cell count must be a non-negative integer")
        if not isinstance(assets, dict):
            raise TypeError("marimo runtime assets must be an object")
        if not isinstance(notebook_code, str):
            raise TypeError("marimo notebook code must be a string")
        return cls(
            id=app_id,
            runtime_cell_count=runtime_cell_count,
            assets=dict(assets),
            notebook_code=notebook_code,
        )


@dataclass(frozen=True)
class CompiledMarimoPage:
    app: MarimoPageRuntime | None
    cells: tuple[CompiledMarimoCell, ...]
    diagnostics: tuple[JsonObject, ...] = ()
    protocol_version: int = PAGE_PROTOCOL_VERSION

    def to_json(self) -> JsonObject:
        return {
            "protocolVersion": self.protocol_version,
            "app": self.app.to_json() if self.app else None,
            "cells": [cell.to_json() for cell in self.cells],
            "diagnostics": list(self.diagnostics),
        }

    @classmethod
    def from_json(cls, value: Any) -> CompiledMarimoPage:
        payload = require_object(value, "compiled marimo page")
        version = payload.get("protocolVersion")
        if version != PAGE_PROTOCOL_VERSION:
            raise ValueError(f"unsupported marimo page protocol: {version}")
        app = payload.get("app")
        cells = payload.get("cells")
        diagnostics = payload.get("diagnostics")
        if diagnostics is None:
            diagnostics = []
        if not isinstance(cells, list):
            raise TypeError("compiled marimo cells must be an array")
        if not isinstance(diagnostics, list) or not all(
            isinstance(item, dict) for item in diagnostics
        ):
            raise TypeError("compiled marimo diagnostics must be an array")
        return cls(
            app=MarimoPageRuntime.from_json(app) if app is not None else None,
            cells=tuple(CompiledMarimoCell.from_json(cell) for cell in cells),
            diagnostics=tuple(dict(item) for item in diagnostics),
        )


def require_object(value: Any, name: str) -> JsonObject:
    if not isinstance(value, dict):
        raise TypeError(f"{name} must be an object")
    return dict(value)


def optional_int(value: Any, name: str) -> int | None:
    if value is None:
        return None
    if not isinstance(value, int) or isinstance(value, bool):
        raise TypeError(f"{name} must be an integer")
    return value
