#!/usr/bin/env python3

"""Execute Quarto marimo cells and build the bundle the engine consumes.

This module is the Python side of the extension: it parses the document with
marimo's markdown pipeline, builds the app once, and returns a shared page
header plus one rendered payload per marimo block.

For HTML output the header also carries the exported notebook source and a
trusted export marker. That gives islands the original notebook back during
hydration, including document-level metadata that would be lost in a pure
cell-by-cell reconstruction.
"""

import asyncio
import json
import keyword
import os
import re
import sys
from collections.abc import Callable
from dataclasses import dataclass
from textwrap import dedent
from typing import Any, ClassVar, Optional
from urllib.parse import quote

# Native to python
from xml.etree.ElementTree import Element

import marimo
from marimo import App, MarimoIslandGenerator
from marimo._convert.common.format import sql_to_marimo
from marimo._session.notebook import AppFileManager

try:
    from marimo._convert.markdown.to_ir import (
        MARIMO_MD,
        MarimoMdParser as MarimoParser,
        SafeWrap as SafeWrapGeneric,
    )
except ImportError:
    from marimo._convert.markdown.markdown import (  # type: ignore[no-redef]
        MARIMO_MD,
        MarimoMdParser as MarimoParser,
        SafeWrap as SafeWrapGeneric,
    )

from marimo._islands import MarimoIslandStub

SafeWrap = SafeWrapGeneric[App]


@dataclass(frozen=True, slots=True)
class _ParseError:
    """Sentinel for a cell whose source couldn't be added to the app.

    Surfaces parse-time exceptions (e.g. `SyntaxError`) that would otherwise be
    swallowed by the broad except around `app.add_code` and rendered as empty
    HTML, leaving the reader no clue that a cell was dropped. (Linear MO-6287.)
    """

    message: str


__version__ = "0.4.5"

# See https://quarto.org/docs/computations/execution-options.html
default_config = {
    "eval": True,
    "echo": False,
    "output": True,
    "warning": True,
    "error": True,
    "include": True,
    # Particular to marimo
    "editor": False,
}

# Rewrite md-form SQL fences (both dot-joined and class-with-space) to the
# qmd-form `sql {.marimo ...}` shape that marimo's parser classifies as
# language="sql". Without this both `{sql.marimo}` and `{sql .marimo}` are
# parsed as language="python", drop into `app.add_code` as raw SQL, and emit
# empty cells. (Linear MO-6285.)
SQL_DOT_FENCE_REGEX = re.compile(
    r"^(\s*`{3,})\s*\{\s*sql\s*\.marimo(?P<attrs>[^}]*)\}\s*$",
    re.MULTILINE,
)

# Rewrite a BARE `{sql ...}` fence (no `.marimo` marker anywhere) to the
# same qmd-form `sql {.marimo ...}` shape SQL_DOT_FENCE_REGEX produces, so
# marimo's parser classifies it as language="sql" instead of misclassifying
# it as python. This is q2's bare-sql Interop feature (Option B, plan4c
# corrections 3/9/12): bare sql rides along as marimo-owned only when q2's
# resolver says so (threaded in via the `bare_sql` argv flag, B2) — see
# `rewrite_bare_sql` below, which gates application on that flag.
#
# Lookaheads (mirroring SQL_DOT_FENCE_REGEX's exclusions, spike-proven
# 2026-07-02):
#   (?![\w.])         -- don't match `{sqlfoo}` (word char right after "sql")
#                         or `{sql.marimo}` (dot right after "sql")
#   (?![^}]*\.marimo)  -- don't match a fence that already carries `.marimo`
#                         anywhere before the closing brace, e.g.
#                         `{sql .marimo}` (class-form with a space)
# The `\{` requirement (no `\{?` optional-brace escape hatch like
# MARIMO_CELL_REGEX) means already-qmd-form `sql {.marimo}` (language before
# the brace) never matches this pattern at all, so it's left untouched too.
BARE_SQL_FENCE_REGEX = re.compile(
    r"^(\s*`{3,})\s*\{\s*sql(?![\w.])(?![^}]*\.marimo)(?P<attrs>[^}]*)\}\s*$",
    re.MULTILINE,
)
DEFAULT_SQL_QUERY_TARGET = "_df"


def is_valid_python_identifier(name: str) -> bool:
    return name.isidentifier() and not keyword.iskeyword(name)


def sql_query_target(query: Optional[str]) -> str:
    if query and is_valid_python_identifier(query):
        return query
    return DEFAULT_SQL_QUERY_TARGET


def is_true_attr(value: Optional[str]) -> bool:
    return str(value or "false").lower() == "true"


def sql_code_to_python(
    code: str,
    query: Optional[str],
    hide_output: bool = False,
    engine: Optional[str] = None,
) -> str:
    """Convert a marimo markdown SQL cell into executable Python."""
    if engine is not None and not is_valid_python_identifier(engine):
        sys.stderr.write(
            f"marimo: ignoring invalid SQL engine name {engine!r} "
            "(must be a Python identifier); falling back to default engine\n"
        )
        engine = None
    return sql_to_marimo(code, sql_query_target(query), hide_output, engine)


def extract_and_strip_quarto_config(block: str) -> tuple[dict[str, Any], str]:
    pattern = r"^\s*\#\|\s*(.*?)\s*:\s*(.*?)(?=\n|\Z)"
    config: dict[str, Any] = {}
    lines = block.split("\n")
    if not lines:
        return config, block

    split_index = 0
    for i, line in enumerate(lines):
        split_index = i
        line = line.strip()
        if not line:
            continue
        source_match = re.search(pattern, line)
        if not source_match:
            break
        key, value = source_match.groups()
        config[key] = json.loads(value)
    return config, "\n".join(lines[split_index:])


def get_mime_render(
    global_options: dict[str, Any],
    stub: Optional[MarimoIslandStub | _ParseError],
    config: dict[str, bool],
    mime_sensitive: bool,
) -> dict[str, Any]:
    """Map one executed island stub to the payload Quarto should splice in.

    This is where the export path forks between browser-hydrated HTML and
    MIME-sensitive outputs like PDF. For HTML we prefer marimo's island HTML so
    the page can hydrate later; for MIME-sensitive targets we degrade to static
    figures or text because there is no browser runtime to finish the job.
    """
    # Local supersede global supersedes default options
    config = {**global_options, **config}
    if not config["include"] or stub is None:
        return {"type": "html", "value": ""}

    if isinstance(stub, _ParseError):
        if not config["error"]:
            return {"type": "html", "value": ""}
        if mime_sensitive:
            return {"type": "blockquote", "value": stub.message}
        return {
            "type": "html",
            "value": f'<pre class="marimo-error">{stub.message}</pre>',
        }

    eval_enabled = config["eval"]
    show_output = config["output"] and eval_enabled
    output = stub.output
    render_options = {
        "display_code": config["echo"],
        "reactive": eval_enabled and not mime_sensitive,
        "code": stub.code,
    }

    if output:
        mimetype = output.mimetype
        if show_output and mime_sensitive:
            if mimetype.startswith("image"):
                return {"type": "figure", "value": f"{output.data}", **render_options}
            # Handle mimebundle - extract image data if present
            if mimetype == "application/vnd.marimo+mimebundle":
                try:
                    bundle_data = (
                        json.loads(output.data)
                        if isinstance(output.data, str)
                        else output.data
                    )
                    if not isinstance(bundle_data, dict):
                        raise TypeError("Expected mimebundle dictionary")
                    bundle: dict[str, Any] = bundle_data
                    # Look for image data in the bundle
                    for key in ["image/png", "image/jpeg", "image/svg+xml"]:
                        if key in bundle:
                            return {
                                "type": "figure",
                                "value": bundle[key],
                                **render_options,
                            }
                    # Fall back to text if no image
                    if "text/plain" in bundle:
                        return {
                            "type": "para",
                            "value": bundle["text/plain"],
                            **render_options,
                        }
                except (json.JSONDecodeError, TypeError):
                    pass  # Fall through to default handling
            if mimetype.startswith(("text/plain", "text/markdown")):
                return {"type": "para", "value": f"{output.data}", **render_options}
            if mimetype == "application/vnd.marimo+error":
                if config["error"]:
                    return {
                        "type": "blockquote",
                        "value": f"{output.data}",
                        **render_options,
                    }
                # Suppress errors otherwise
                return {"type": "para", "value": "", **render_options}

        elif mimetype == "application/vnd.marimo+error":
            if config["warning"]:
                sys.stderr.write(
                    "Warning: Only the `disabled` codeblock attribute is utilized"
                    " for pandoc export. Be sure to set desired code attributes "
                    "in quarto form."
                )
            if not config["error"]:
                return {"type": "html", "value": ""}

    # Nothing to display (e.g. eval=false with echo=false)
    if not config["echo"] and not show_output:
        return {"type": "html", "value": "", **render_options}

    # HTML as catch all default
    return {
        "type": "html",
        "value": stub.render(
            display_code=config["echo"],
            display_output=show_output,
            is_reactive=bool(render_options["reactive"]),
            as_raw=mime_sensitive,
        ),
        **render_options,
    }


def app_config_from_root(root: Element) -> dict[str, Any]:
    # Extract meta data from root attributes.
    config_keys = {"title": "app_title", "marimo-layout": "layout_file"}
    config = {
        config_keys[key]: value for key, value in root.items() if key in config_keys
    }
    # Try to pass on other attributes as is
    config.update({k: v for k, v in root.items() if k not in config_keys})
    # Remove values particular to markdown saves.
    config.pop("marimo-version", None)
    return config


def pyproject_to_script_metadata(pyproject: str) -> str:
    """Lift Quarto frontmatter into the top-of-file metadata marimo exports use.

    The browser-side islands runtime only sees notebook source, not the
    original YAML frontmatter. Re-encoding the document-level ``pyproject`` as
    script metadata lets the exported notebook carry dependency declarations in
    the one place marimo already knows how to read during hydration.
    """
    body = dedent(pyproject).strip()
    if not body:
        return ""
    if body.startswith("# /// script"):
        return f"{body}\n"

    commented_lines = ["# /// script"]
    for line in body.splitlines():
        commented_lines.append(f"# {line}" if line else "#")
    commented_lines.append("# ///")
    return "\n".join(commented_lines) + "\n"


def build_export_notebook_code(app: MarimoIslandGenerator, script_metadata: str) -> str:
    """Serialize the executed app as a standalone marimo notebook source string."""
    notebook_code = AppFileManager.from_app(app._app).to_code()
    if script_metadata:
        notebook_code = f"{script_metadata}{notebook_code}"
    return notebook_code


def render_hidden_marimo_code(notebook_code: str) -> str:
    return f"<marimo-code hidden>{quote(notebook_code, safe='')}</marimo-code>"


def json_script(value: str) -> str:
    return (
        json.dumps(value)
        .replace("<", "\\u003C")
        .replace(">", "\\u003E")
        .replace("&", "\\u0026")
    )


def render_export_context_script(notebook_code: str) -> str:
    """Emit the trusted export marker consumed by marimo islands at runtime.

    The hidden notebook source alone is not a trust signal because page markup
    can be spoofed. This script installs a runtime-owned context object that
    marimo core can validate before it trusts export-only affordances such as
    inline virtual-file ``data:`` URLs.
    """
    return dedent(
        f"""
        <script data-marimo="true">
            Object.defineProperty(window, "__MARIMO_EXPORT_CONTEXT__", {{
                value: Object.freeze({{
                    trusted: true,
                    notebookCode: {json_script(notebook_code)},
                }}),
                writable: false,
                configurable: false,
            }});
        </script>
        """
    ).strip()


def build_export_with_mime_context(
    mime_sensitive: bool,
) -> Callable[[Element], SafeWrap]:  # type: ignore[valid-type]
    """Create the parser callback that turns a Quarto AST into export payloads.

    Quarto asks for two variants of the same document:
    (1) an HTML-oriented one for browser output and
    (2) a MIME-sensitive one for formats like PDF.

    The callback keeps those paths together so both are derived from one executed marimo app,
    while only the HTML path receives notebook provenance and hydration metadata.
    """

    def tree_to_pandoc_export(root: Element) -> SafeWrap:  # type: ignore[valid-type]
        global_options = {**default_config, **app_config_from_root(root)}
        app = MarimoIslandGenerator()

        has_attrs: bool = False
        stubs: list[
            tuple[dict[str, bool], Optional[MarimoIslandStub | _ParseError]]
        ] = []
        for child in root:
            # only process code cells
            if child.tag == MARIMO_MD:
                continue
            # We only care about the disabled attribute.
            if child.attrib.get("disabled") == "true":
                # Don't even add to generator
                stubs.append(({"include": False}, None))
                continue
            # Check to see id attrs are defined on the tag
            has_attrs = has_attrs | bool(child.attrib.items())

            code = str(child.text)
            config, code = extract_and_strip_quarto_config(code)
            if child.attrib.get("language") == "sql":
                code = sql_code_to_python(
                    code,
                    child.attrib.get("query"),
                    hide_output=is_true_attr(child.attrib.get("hide_output")),
                    engine=child.attrib.get("engine"),
                )

            try:
                stub = app.add_code(
                    code,
                    is_raw=True,
                )
            except Exception as exc:  # noqa: BLE001
                msg = f"{type(exc).__name__}: {exc}"
                sys.stderr.write(f"marimo: failed to parse cell: {msg}\n")
                stubs.append((config, _ParseError(msg)))
                continue

            assert isinstance(stub, MarimoIslandStub), "Unexpected error, please report"

            stubs.append(
                (
                    config,
                    stub,
                )
            )

        if has_attrs and global_options.get("warning", True):
            pass

        if global_options.get("eval", True):
            _ = asyncio.run(app.build())
        dev_server = os.environ.get("QUARTO_MARIMO_DEBUG_ENDPOINT") or False
        version_override = os.environ.get("QUARTO_MARIMO_VERSION", marimo.__version__)
        header = app.render_head(
            _development_url=dev_server, version_override=version_override
        )
        outputs = [
            get_mime_render(global_options, stub, config, mime_sensitive)
            for config, stub in stubs
        ]
        if not mime_sensitive:
            script_metadata = pyproject_to_script_metadata(
                str(global_options.get("pyproject", ""))
            )
            notebook_code = build_export_notebook_code(app, script_metadata)
            header = (
                render_export_context_script(notebook_code)
                + render_hidden_marimo_code(notebook_code)
                + header
            )

        return SafeWrap(  # type: ignore[no-any-return]
            {
                "header": header,
                "outputs": outputs,
                "count": len(stubs),
            }  # type: ignore[arg-type]
        )

    return tree_to_pandoc_export


class MarimoPandocParser(MarimoParser):  # type: ignore[misc]
    """Parses Markdown to marimo notebook string."""

    # TODO: Could upstream generic for keys- but this is fine.
    output_formats: ClassVar[dict[str, Any]] = {  # type: ignore[assignment, misc]
        "marimo-pandoc-export": build_export_with_mime_context(mime_sensitive=False),  # type: ignore[dict-item]
        "marimo-pandoc-export-with-mime": build_export_with_mime_context(
            mime_sensitive=True
        ),  # type: ignore[dict-item]
    }


def rewrite_bare_sql(text: str, enabled: bool) -> str:
    """Rewrite bare `{sql ...}` fences into marimo's qmd-form, when enabled.

    Factored out of `convert_from_md_to_pandoc_export` (q2 plan4c 4c0-eng,
    SC6) so the regex behavior is unit-testable without going through
    marimo's parser. `enabled` is threaded from the TS engine's `bare_sql`
    argv flag (argv[4], B2), itself derived from
    `handledLanguages.includes("sql")` — bare sql should only be rewritten
    (and thus executed as marimo SQL) when q2's resolver has determined
    marimo owns it for this render. When disabled, bare `{sql}` fences are
    left as plain python-classified cells (today's pre-4c0-eng behavior),
    which will error/no-op rather than silently execute under someone else's
    ownership.
    """
    if not enabled:
        return text
    return BARE_SQL_FENCE_REGEX.sub(r"\1sql {.marimo\g<attrs>}", text)


def convert_from_md_to_pandoc_export(
    text: str, mime_sensitive: bool, bare_sql: bool = False
) -> dict[str, Any]:
    """Entry point used by the engine subprocess contract.

    The TypeScript engine sends full markdown over stdin and expects a JSON bundle back.
    Keeping that contract in one function makes the extension easier to test directly from Python
    without going through Quarto itself.
    """
    if not text:
        return {"header": "", "outputs": []}
    text = rewrite_bare_sql(text, bare_sql)
    text = SQL_DOT_FENCE_REGEX.sub(r"\1sql {.marimo\g<attrs>}", text)
    if mime_sensitive:
        parser = MarimoPandocParser(output_format="marimo-pandoc-export-with-mime")  # type: ignore[arg-type]
    else:
        parser = MarimoPandocParser(output_format="marimo-pandoc-export")  # type: ignore[arg-type]
    return parser.convert(text)  # type: ignore[arg-type, return-value, no-any-return]


if __name__ == "__main__":
    # argv: [script, reference_file, mime_sensitive, global_eval?, bare_sql?]
    # `bare_sql` (argv[4]) is a NEW 4th positional (q2 plan4c 4c0-eng, B2) —
    # widened from (3, 4) to (3, 4, 5) so the TS engine's new argument
    # doesn't hit this assert at runtime past SC6's pure-regex tests.
    assert len(sys.argv) in (3, 4, 5), f"Unexpected call format got {sys.argv}"
    _, reference_file, mime_sensitive = sys.argv[:3]
    global_eval = sys.argv[3].lower() == "yes" if len(sys.argv) >= 4 else True
    bare_sql = sys.argv[4].lower() == "yes" if len(sys.argv) >= 5 else False

    file = sys.stdin.read()
    if not file:
        with open(reference_file) as f:
            file = f.read()
    no_js = mime_sensitive.lower() == "yes"
    os.environ["MARIMO_NO_JS"] = str(no_js).lower()

    if not global_eval:
        default_config["eval"] = False

    conversion = convert_from_md_to_pandoc_export(file, no_js, bare_sql)
    sys.stdout.write(json.dumps(conversion))
