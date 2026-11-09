# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "marimo>=0.23.15",
# ]
# ///
"""Compile one publishing page into the marimo islands page protocol."""

from __future__ import annotations

import ast
import asyncio
import hashlib
import json
import keyword
import re
import symtable
import sys
from contextlib import redirect_stdout
from dataclasses import dataclass
from html.parser import HTMLParser
from importlib import import_module
from typing import Any, cast
from urllib.parse import quote

import marimo
from marimo import MarimoIslandGenerator
from marimo._ast.codegen import generate_filecontents_from_ir
from marimo._convert.common.format import markdown_to_marimo, sql_to_marimo
from marimo._schemas.serialization import (
    AppInstantiation,
    CellDef,
    NotebookSerializationV1,
    UnparsableCell,
)

PAGE_PROTOCOL_VERSION = 2
DEFAULT_MARIMO_IMPORT = "import marimo as mo"
ERROR_MIMETYPES = {
    "application/vnd.marimo+error",
    "application/vnd.marimo+traceback",
}
DEFAULT_CELL_OPTIONS: dict[str, Any] = {
    "language": "python",
    "render": {
        "source": False,
        "output": True,
        "include": True,
        "editor": False,
        "error": True,
        "serverOutput": True,
    },
    "execution": {"enabled": True},
    "marimo": {"disabled": False, "unparsable": False},
}


class HeadAssetParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.module_scripts: list[str] = []
        self.links: list[dict[str, str]] = []
        self.head_tags: list[dict[str, Any]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = {key: value or "" for key, value in attrs}
        if tag == "script" and values.get("type") == "module" and values.get("src"):
            self.module_scripts.append(values["src"])
        elif tag == "link" and values.get("href"):
            self.links.append(values)
        elif tag != "script":
            self.head_tags.append({"tag": tag, "attrs": values})


@dataclass
class PlannedCell:
    index: int
    code: str
    options: dict[str, Any]
    executable_source: str
    execute: bool
    display_code: bool
    display_editor: bool
    display_output: bool
    display_server_output: bool
    start_line: int | None


@dataclass
class PageRequest:
    filename: str
    app_id: str
    pyproject: str | None
    setup_cells: list[PlannedCell]
    cells: list[PlannedCell]

    @property
    def runtime_cells(self) -> list[PlannedCell]:
        return [*self.setup_cells, *self.cells]


def as_bool(value: Any, default: bool = False) -> bool:
    if value is None:
        return default
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.lower() == "true"
    return bool(value)


def as_dict(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict):
        return {}
    return cast(dict[str, Any], value)


def as_list(value: Any) -> list[Any]:
    if not isinstance(value, list):
        return []
    return value


def is_valid_identifier(name: str) -> bool:
    return name.isidentifier() and not keyword.iskeyword(name)


def sql_target(output_name: Any) -> str:
    target = str(output_name or "_df")
    return target if is_valid_identifier(target) and target != "mo" else "_df"


def sql_engine(value: Any) -> str | None:
    if value is None:
        return None
    engine = str(value)
    if is_valid_identifier(engine):
        return engine
    sys.stderr.write(
        f"marimo: ignoring invalid SQL engine name {engine!r} "
        "(must be a Python identifier); falling back to the default engine\n"
    )
    return None


def executable_source(cell: dict[str, Any], options: dict[str, Any]) -> str:
    code = str(cell.get("source") or "")
    language = str(options.get("language") or "python").lower()
    if language == "sql":
        sql = as_dict(options.get("sql"))
        render = as_dict(options.get("render"))
        return sql_to_marimo(
            code,
            sql_target(sql.get("outputName")),
            not as_bool(render.get("output"), True),
            sql_engine(sql.get("engine")),
        )
    if language == "markdown":
        return markdown_to_marimo(code)
    return code


def merge_cell_options(
    defaults: dict[str, Any], cell_options: dict[str, Any]
) -> dict[str, Any]:
    options = dict(DEFAULT_CELL_OPTIONS)
    options.update(defaults)
    options.update(cell_options)
    for section in ("render", "execution", "marimo", "sql"):
        merged: dict[str, Any] = {}
        for source in (DEFAULT_CELL_OPTIONS, defaults, cell_options):
            value = source.get(section)
            if isinstance(value, dict):
                merged.update(value)
        if merged:
            options[section] = merged
    return options


def plan_cell(
    cell: dict[str, Any], defaults: dict[str, Any], *, setup: bool = False
) -> PlannedCell:
    cell_options = dict(as_dict(cell.get("options")))
    options = merge_cell_options(defaults, cell_options)
    render = as_dict(options.get("render"))
    execution = as_dict(options.get("execution"))
    marimo_options = as_dict(options.get("marimo"))
    cell_render = as_dict(cell_options.get("render"))
    default_render = as_dict(defaults.get("render"))
    if as_bool(render.get("editor")):
        render["source"] = True
    if (
        not setup
        and as_bool(marimo_options.get("unparsable"))
        and "source" not in cell_render
        and "source" not in default_render
    ):
        render["source"] = True
    include = False if setup else as_bool(render.get("include"), True)
    execute = (
        as_bool(execution.get("enabled"), True)
        and not as_bool(marimo_options.get("disabled"))
        and not as_bool(marimo_options.get("unparsable"))
    )
    execution["enabled"] = execute
    display_code = False if setup else include and as_bool(render.get("source"), False)
    display_editor = (
        False if setup else include and as_bool(render.get("editor"), False)
    )
    display_output = include and as_bool(render.get("output"), True)
    display_server_output = display_output and as_bool(render.get("serverOutput"), True)
    return PlannedCell(
        index=int(cell.get("index") or 0),
        code=str(cell.get("source") or ""),
        options=options,
        executable_source=executable_source(cell, options),
        execute=execute,
        display_code=display_code,
        display_editor=display_editor,
        display_output=display_output,
        display_server_output=display_server_output,
        start_line=(
            int(cell["startLine"])
            if isinstance(cell.get("startLine"), int)
            and not isinstance(cell.get("startLine"), bool)
            else None
        ),
    )


def page_request(payload: dict[str, Any]) -> PageRequest:
    protocol_version = payload.get("protocolVersion")
    if protocol_version != PAGE_PROTOCOL_VERSION:
        raise ValueError(f"unsupported marimo page protocol: {protocol_version}")
    identity = str(payload.get("identity") or payload.get("filename") or "document")
    filename = str(payload.get("filename") or "")
    metadata = as_dict(payload.get("metadata"))
    pyproject = (
        metadata.get("pyproject")
        if isinstance(metadata.get("pyproject"), str)
        else None
    )
    defaults = as_dict(payload.get("defaults"))
    setup_cells = as_list(metadata.get("setupCells"))
    authored_cells = [
        plan_cell(cell, defaults)
        for cell in as_list(payload.get("cells"))
        if isinstance(cell, dict)
    ]
    explicit_setup_cells = [
        plan_cell(cell, defaults, setup=True)
        for cell in setup_cells
        if isinstance(cell, dict)
    ]
    executable_setup_cells = [cell for cell in explicit_setup_cells if cell.execute]
    return PageRequest(
        filename=filename,
        app_id="marimo-" + page_digest(identity),
        pyproject=pyproject,
        setup_cells=[
            *generated_setup_cells(executable_setup_cells, authored_cells),
            *executable_setup_cells,
        ],
        cells=authored_cells,
    )


def page_digest(identity: str) -> str:
    return hashlib.sha256(identity.encode("utf-8")).hexdigest()[:12]


def generated_setup_cells(
    setup_cells: list[PlannedCell],
    authored_cells: list[PlannedCell],
) -> list[PlannedCell]:
    setup_sources = [cell.executable_source for cell in setup_cells]
    generated_cells = [
        cell
        for cell in authored_cells
        if cell.execute and cell.options.get("language") in {"markdown", "sql"}
    ]
    generated_sources = [cell.executable_source for cell in generated_cells]
    import_sources = [
        *setup_sources,
        *(cell.executable_source for cell in authored_cells if cell.execute),
    ]
    if not any(
        source_uses_name(source, "mo")
        for source in [*setup_sources, *generated_sources]
    ):
        return []
    if any(
        source_imports_marimo_module_alias(source, "mo") for source in import_sources
    ):
        return []
    if any(source_defines_name(source, "mo") for source in import_sources):
        # A separate setup import would define `mo` in a second notebook cell.
        # Keep the compiler-owned alias private within each generated cell.
        for cell in generated_cells:
            cell.executable_source = source_with_private_marimo_alias(
                cell.executable_source,
                "mo",
            )
        return []
    return [
        plan_cell(
            {
                "index": -1,
                "source": DEFAULT_MARIMO_IMPORT,
                "options": {"language": "python"},
            },
            {},
            setup=True,
        )
    ]


def assets_from_head(head: str, filename: str = "") -> dict[str, Any]:
    parser = HeadAssetParser()
    parser.feed(head)
    if filename:
        for tag in parser.head_tags:
            if tag.get("tag") == "marimo-filename":
                tag["text"] = quote(filename)
    return {
        "moduleScripts": parser.module_scripts,
        "links": parser.links,
        "headTags": parser.head_tags,
    }


def renders_author_source(plan: PlannedCell) -> bool:
    language = str(plan.options.get("language") or "python").lower()
    return plan.display_code and (language != "python" or not plan.display_editor)


def cell_name(plan: PlannedCell) -> str:
    name = plan.options.get("name")
    return str(name) if isinstance(name, str) and is_valid_identifier(name) else "_"


def cell_config(plan: PlannedCell) -> dict[str, Any]:
    config: dict[str, Any] = {}
    column = plan.options.get("column")
    if isinstance(column, int):
        config["column"] = column
    if not plan.execute:
        config["disabled"] = True
    return config


def ir_cell(plan: PlannedCell) -> CellDef:
    marimo_options = as_dict(plan.options.get("marimo"))
    cell_type = UnparsableCell if as_bool(marimo_options.get("unparsable")) else CellDef
    code = plan.code if cell_type is UnparsableCell else plan.executable_source
    return cell_type(
        code=code,
        name=cell_name(plan),
        options=cell_config(plan),
    )


def to_marimo_ir(request: PageRequest) -> NotebookSerializationV1:
    # The source path remains available for diagnostics and runtime metadata.
    # The generated notebook uses no filename because publishing source files
    # are not necessarily valid marimo notebook filenames.
    return NotebookSerializationV1(
        app=AppInstantiation(options={}),
        filename=None,
        cells=[ir_cell(plan) for plan in request.runtime_cells],
    )


def generator_from_ir(
    notebook: NotebookSerializationV1, request: PageRequest
) -> MarimoIslandGenerator:
    if notebook.cells:
        return MarimoIslandGenerator._from_ir(
            notebook,
            app_id=request.app_id,
            filepath=request.filename or None,
        )
    return MarimoIslandGenerator(app_id=request.app_id)


async def build_generator(generator: MarimoIslandGenerator) -> None:
    from marimo._session.notebook import AppFileManager

    if getattr(generator, "has_run", False):
        raise ValueError("marimo island generator has already been built")

    # Publishing builds should not create marimo session files beside sources.
    file_manager = AppFileManager.from_app(
        generator._app,
        filename=getattr(generator, "_source_filename", None),
    )
    # marimo 0.23.16 moved notebook execution to request objects.
    try:
        export_file = import_module("marimo._export.file")
        export_requests = import_module("marimo._export.requests")
    except ModuleNotFoundError as error:
        if error.name not in {
            "marimo._export",
            "marimo._export.file",
            "marimo._export.requests",
        }:
            raise
        server_export = import_module("marimo._server.export")

        session, _did_error = await server_export.run_app_until_completion(
            file_manager=file_manager,
            cli_args={},
            argv=None,
            quiet=True,
            persist_session=False,
        )
    else:
        session, _did_error = await export_file.run_notebook(
            export_requests.RunNotebookRequest(
                file_manager=file_manager,
                options=export_requests.NotebookExecutionOptions(
                    cli_args={},
                    argv=None,
                    quiet=True,
                    persist_session=False,
                ),
            )
        )
    generator.has_run = True

    for stub in generator.stubs:
        stub._internal_app = generator._app
        stub._session_view = session


def stubs_from_generator(
    generator: MarimoIslandGenerator, plans: list[PlannedCell]
) -> list[Any]:
    generated_stubs = list(generator.stubs)
    if len(generated_stubs) != len(plans):
        raise RuntimeError(
            "marimo IR compilation produced an unexpected island stub count"
        )
    return generated_stubs


def assets_from_generator(
    generator: MarimoIslandGenerator,
    request: PageRequest,
    *,
    development_url: str | None = None,
    version_override: str | None = None,
) -> dict[str, Any]:
    if development_url is None and version_override is None:
        head = generator.render_head()
    else:
        effective_version = version_override
        if effective_version is None:
            installed_version = getattr(marimo, "__version__", "")
            effective_version = (
                installed_version if isinstance(installed_version, str) else ""
            )
        head = generator.render_head(
            _development_url=development_url or False,
            version_override=effective_version,
        )
    assets = assets_from_head(head, request.filename)
    version = getattr(marimo, "__version__", None)
    if isinstance(version, str) and version:
        assets["version"] = version
    return assets


def browser_notebook_code(
    notebook: NotebookSerializationV1, pyproject: str | None
) -> str:
    parts: list[str] = []
    metadata = inline_script_metadata(pyproject)
    if metadata:
        parts.append(metadata)
    parts.append(generate_filecontents_from_ir(notebook).strip())
    return "\n".join(parts)


def inline_script_metadata(pyproject: str | None) -> str:
    if not pyproject or not pyproject.strip():
        return ""
    body = pyproject.strip()
    if body.startswith("# /// script"):
        return body
    lines = ["# /// script"]
    for line in body.splitlines():
        lines.append(f"# {line}" if line else "#")
    lines.append("# ///")
    return "\n".join(lines)


def author_source_html(plan: PlannedCell) -> str:
    language = str(plan.options.get("language") or "python").lower()
    return (
        f'<pre><code class="language-{escape_html(language)}">'
        f"{escape_html(plan.code)}</code></pre>"
    )


def outputs_from_stubs(
    request: PageRequest,
    plans: list[PlannedCell],
    stubs: list[Any],
) -> list[dict[str, Any]]:
    outputs = []
    for plan, stub in zip(plans, stubs, strict=True):
        render = as_dict(plan.options.get("render"))
        if not as_bool(render.get("error"), True) and has_error_output(stub):
            raise RuntimeError(
                f"marimo execution failed in {error_location(request, plan)}"
            )
        if renders_author_source(plan):
            parts = [author_source_html(plan)]
            if plan.display_output or plan.execute:
                parts.append(
                    render_stub(
                        stub,
                        display_code=False,
                        display_output=plan.display_server_output,
                    )
                )
            html = "\n".join(part for part in parts if part)
        else:
            html = render_stub(
                stub,
                display_code=plan.display_editor,
                display_output=plan.display_server_output,
            )
        outputs.append(
            {
                "html": html,
                "index": plan.index,
                "options": plan.options,
                "output": compiled_output(stub),
            }
        )
    return outputs


def compiled_output(stub: Any) -> dict[str, Any] | None:
    output = stub.output
    if output is None:
        return None
    mimetype = str(output.mimetype)
    data = output.asdict()["data"]
    if mimetype in {"text/html", "image/svg+xml"} and isinstance(data, str):
        html = data
    else:
        html = render_stub(
            stub,
            display_code=False,
            display_output=True,
            is_reactive=False,
            as_raw=True,
        )
    return {
        "mimetype": mimetype,
        "data": data,
        "html": html,
    }


def render_stub(
    stub: Any,
    *,
    display_code: bool,
    display_output: bool,
    is_reactive: bool | None = None,
    as_raw: bool = False,
) -> str:
    try:
        return str(
            stub.render(
                display_code=display_code,
                display_output=display_output,
                is_reactive=is_reactive,
                as_raw=as_raw,
            )
        )
    except ValueError:
        if not display_code and not display_output:
            return ""
        raise


def has_error_output(stub: Any) -> bool:
    output = stub.output
    return output is not None and str(output.mimetype) in ERROR_MIMETYPES


def error_location(request: PageRequest, plan: PlannedCell) -> str:
    filename = request.filename or "marimo page"
    return f"{filename}:{plan.start_line}" if plan.start_line is not None else filename


def source_imports_marimo_module_alias(source: str, name: str) -> bool:
    if not source.strip():
        return False
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return False
    for node in tree.body:
        if isinstance(node, ast.Import):
            if any(
                alias.name == "marimo" and (alias.asname or alias.name) == name
                for alias in node.names
            ):
                return True
    return False


def source_defines_name(source: str, name: str) -> bool:
    if not source.strip():
        return False
    try:
        symbols = symtable.symtable(source, "<marimo-cell>", "exec")
    except SyntaxError:
        return False
    if name not in symbols.get_identifiers():
        return False
    symbol = symbols.lookup(name)
    return symbol.is_assigned() or symbol.is_imported()


def source_with_private_marimo_alias(source: str, name: str) -> str:
    tree = ast.parse(source)
    names = {node.id for node in ast.walk(tree) if isinstance(node, ast.Name)}
    alias = f"_{name}"
    while alias in names:
        alias = f"_{alias}"
    statement = tree.body[0]
    expression = (
        statement.value if isinstance(statement, (ast.Assign, ast.Expr)) else None
    )
    if (
        isinstance(expression, ast.Call)
        and isinstance(expression.func, ast.Attribute)
        and expression.func.attr in {"md", "sql"}
        and isinstance(expression.func.value, ast.Name)
        and expression.func.value.id == name
    ):
        expression.func.value.id = alias
    return f"import marimo as {alias}\n{ast.unparse(tree)}"


def source_uses_name(source: str, name: str) -> bool:
    if not source.strip():
        return False
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return bool(re.search(rf"\b{re.escape(name)}\b", source))
    return any(
        isinstance(node, ast.Name)
        and isinstance(node.ctx, ast.Load)
        and node.id == name
        for node in ast.walk(tree)
    )


async def compile_page(
    payload: dict[str, Any],
    *,
    development_url: str | None = None,
    version_override: str | None = None,
) -> dict[str, Any]:
    request = page_request(payload)
    notebook = to_marimo_ir(request)
    generator = generator_from_ir(notebook, request)
    runtime_cells = request.runtime_cells
    stubs = stubs_from_generator(generator, runtime_cells)

    if stubs:
        await build_generator(generator)

    for plan, stub in zip(
        request.setup_cells,
        stubs[: len(request.setup_cells)],
        strict=True,
    ):
        if has_error_output(stub):
            raise RuntimeError(
                f"marimo execution failed in {error_location(request, plan)}"
            )

    assets = assets_from_generator(
        generator,
        request,
        development_url=development_url,
        version_override=version_override,
    )
    authored_stubs = stubs[len(request.setup_cells) :]
    app = {
        "id": request.app_id,
        "runtimeCellCount": len(runtime_cells),
        "assets": assets,
        "notebookCode": browser_notebook_code(notebook, request.pyproject),
    }
    return {
        "protocolVersion": PAGE_PROTOCOL_VERSION,
        "app": app if runtime_cells else None,
        "cells": outputs_from_stubs(request, request.cells, authored_stubs),
        "diagnostics": [],
    }


def escape_html(value: str) -> str:
    return (
        value.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def main() -> None:
    payload = json.loads(sys.stdin.read())
    with redirect_stdout(sys.stderr):
        result = asyncio.run(compile_page(payload))
    sys.stdout.write(json.dumps(result))


if __name__ == "__main__":
    main()
