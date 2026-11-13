"""Normalize Quarto cell syntax and execution options."""

from __future__ import annotations

import json
import re
from typing import TYPE_CHECKING, Any

from quarto_marimo.protocol import JsonObject

if TYPE_CHECKING:
    from xml.etree.ElementTree import Element

SQL_DOT_FENCE_REGEX = re.compile(
    r"^(\s*`{3,})\s*\{\s*sql\s*\.marimo(?P<attrs>[^}]*)\}\s*$",
    re.MULTILINE,
)

# A bare `{sql ...}` fence (no `.marimo` marker) is rewritten to the same
# qmd-form `sql {.marimo ...}` shape as SQL_DOT_FENCE_REGEX produces, but only
# when Quarto 2's engine resolver assigned bare sql to marimo for this render
# (the `bare_sql` flag the TypeScript engine threads in). Lookaheads:
#   (?![\w.])         -- not `{sqlfoo}` and not `{sql.marimo}`
#   (?![^}]*\.marimo)  -- not a fence that already carries `.marimo`
BARE_SQL_FENCE_REGEX = re.compile(
    r"^(\s*`{3,})\s*\{\s*sql(?![\w.])(?![^}]*\.marimo)(?P<attrs>[^}]*)\}\s*$",
    re.MULTILINE,
)


def rewrite_bare_sql(markdown: str, enabled: bool) -> str:
    if not enabled:
        return markdown
    return BARE_SQL_FENCE_REGEX.sub(r"\1sql {.marimo\g<attrs>}", markdown)


def normalize_markdown(markdown: str, bare_sql: bool = False) -> str:
    markdown = rewrite_bare_sql(markdown, bare_sql)
    return SQL_DOT_FENCE_REGEX.sub(r"\1sql {.marimo\g<attrs>}", markdown)


def page_options_from_root(root: Element) -> JsonObject:
    options = dict(root.items())
    options.pop("marimo-version", None)
    return options


def extract_cell_config(block: str) -> tuple[JsonObject, str]:
    lines = block.splitlines()
    config: JsonObject = {}
    consumed = 0
    for line in lines:
        stripped = line.strip()
        if not stripped:
            consumed += 1
            continue
        match = re.match(r"^\#\|\s*([^:]+?)\s*:\s*(.*?)\s*$", stripped)
        if match is None:
            break
        key, value = match.groups()
        config[key] = parse_option_value(value)
        consumed += 1
    return config, "\n".join(lines[consumed:])


def parse_option_value(value: str) -> Any:
    value = strip_inline_comment(value)
    try:
        return json.loads(value)
    except json.JSONDecodeError:
        normalized = value.strip()
        if normalized.lower() == "true":
            return True
        if normalized.lower() == "false":
            return False
        return normalized.strip("\"'")


def strip_inline_comment(value: str) -> str:
    quote: str | None = None
    escaped = False
    for index, character in enumerate(value):
        if escaped:
            escaped = False
        elif quote == '"' and character == "\\":
            escaped = True
        elif quote is not None and character == quote:
            quote = None
        elif quote is None and character in {'"', "'"}:
            quote = character
        elif (
            quote is None
            and character == "#"
            and (index == 0 or value[index - 1].isspace())
        ):
            return value[:index].rstrip()
    return value.strip()


def cell_options_patch(
    local_options: JsonObject,
    attributes: dict[str, str],
) -> JsonObject:
    values = {
        **local_options,
        **{key.replace("_", "-"): value for key, value in attributes.items()},
    }
    language = str(values.get("language") or "python").strip().lower()
    options = execution_options_patch(values)
    options["language"] = language

    marimo: JsonObject = {}
    disabled = as_bool(values.get("disabled"))
    unparsable = as_bool(values.get("unparsable"))
    if "disabled" in values:
        marimo["disabled"] = disabled
    if "unparsable" in values:
        marimo["unparsable"] = unparsable
    if marimo:
        options["marimo"] = marimo
    if disabled or unparsable:
        execution = options.setdefault("execution", {})
        execution["enabled"] = False
    hide_code = as_bool(values.get("hide-code"))
    if hide_code:
        render = options.setdefault("render", {})
        render["source"] = False
        render["editor"] = False
    elif unparsable:
        render = options.setdefault("render", {})
        render["source"] = True
    if as_bool(values.get("hide-output")):
        render = options.setdefault("render", {})
        render["output"] = False

    if language == "sql":
        sql: JsonObject = {}
        if "query" in values:
            sql["outputName"] = str(values["query"])
        if "engine" in values:
            sql["engine"] = str(values["engine"])
        if sql:
            options["sql"] = sql
    if "name" in values:
        options["name"] = str(values["name"])
    column = optional_int(values.get("column"))
    if column is not None:
        options["column"] = column
    return options


def execution_options_patch(options: JsonObject) -> JsonObject:
    render: JsonObject = {}
    execution: JsonObject = {}
    render_keys = {
        "echo": "source",
        "editor": "editor",
        "output": "output",
        "server-output": "serverOutput",
        "error": "error",
        "include": "include",
    }
    for source, target in render_keys.items():
        if source in options:
            render[target] = as_bool(options[source])
    if as_bool(render.get("editor")):
        render["source"] = True
    if "eval" in options:
        execution["enabled"] = as_bool(options["eval"])

    patch: JsonObject = {}
    if render:
        patch["render"] = render
    if execution:
        patch["execution"] = execution
    return patch


def as_bool(value: Any, default: bool = False) -> bool:
    if value is None:
        return default
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.strip().lower() == "true"
    return bool(value)


def optional_int(value: Any) -> int | None:
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return None
    return parsed
