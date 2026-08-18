/**
 * @quarto/api/jupyter — cell-options
 *
 * Parses the leading `#| key: value` option block out of a code cell's
 * source lines. This is a **simplified** port of Q1's
 * `partitionCellOptions`: no schema validation, no tree-sitter, no
 * "knitr options format" guessing — just prefix-strip the contiguous run of
 * comment-YAML lines at the top of `source` and `yaml.parse` them. Pure
 * module (no host).
 *
 * Ported (simplified rewrite) from:
 *   external-sources/quarto-cli/src/core/lib/partition-cell-options.ts
 *   (`partitionCellOptions`, `langCommentChars`, `optionCommentPattern`)
 *
 * `to-markdown.ts` (Task 8) calls `parseCellOptions` to upgrade a raw
 * `JupyterCell` into a `JupyterCellWithOptions` (attaching `options` +
 * `optionsSource`) before `tags.ts` reads `cell.options`. It derives the
 * remaining code body itself by dropping the leading `optionsSource.length`
 * lines of `source` — this module does not return the code.
 */

import { parse } from "yaml";
import { kLangCommentChars } from "./constants.js";

/**
 * The parsed option block plus the raw matched lines it came from.
 */
export interface ParsedCellOptions {
  /** The parsed `#| key: value` options, or `{}` if there were none/invalid. */
  options: Record<string, unknown>;
  /** The raw, unmodified, contiguous leading lines that matched the option pattern. */
  optionsSource: string[];
}

/**
 * Parse the leading `#| ...` option block out of a code cell's `source`
 * lines for the given cell `language`.
 *
 * Scans `source` from the top and collects the contiguous run of lines
 * matching `^\s*<comment>\s*\|\s?` (the language's line-comment prefix,
 * then `|`, then the option text), stopping at the first non-matching
 * line. Strips the matched prefix from each collected line, joins with
 * `\n`, and `yaml.parse`s the result into `options`. A missing/empty block,
 * or a block that doesn't parse into an object, yields `options: {}` — this
 * function never throws.
 */
export function parseCellOptions(
  source: string[],
  language: string,
): ParsedCellOptions {
  const commentChars = kLangCommentChars[language] ?? "#";
  const commentPrefix = Array.isArray(commentChars)
    ? commentChars[0]
    : commentChars;
  const optionPattern = new RegExp(
    "^\\s*" + escapeRegExp(commentPrefix) + "\\s*\\|\\s?",
  );

  const optionsSource: string[] = [];
  const yamlLines: string[] = [];

  for (const line of source) {
    const match = line.match(optionPattern);
    if (!match) {
      break;
    }
    optionsSource.push(line);
    yamlLines.push(line.substring(match[0].length));
  }

  const options = parseYamlOptions(yamlLines.join("\n"));

  return { options, optionsSource };
}

/** `yaml.parse`, defensively coerced to a plain object; never throws. */
function parseYamlOptions(yamlText: string): Record<string, unknown> {
  if (yamlText.trim().length === 0) {
    return {};
  }
  try {
    const parsed: unknown = parse(yamlText);
    if (parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
    return {};
  } catch {
    return {};
  }
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
