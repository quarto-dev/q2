#!/usr/bin/env node
/**
 * lint:css — dependency-free CSS lint for the hub-client design system.
 *
 * Mirrors the `cargo xtask lint` philosophy: enforce the token-first rules
 * from the UI/UX modernization plan
 * (.posit/assistant/plans/2026-08-25-hub-client-uiux-modernization-plan.md):
 *
 *   no-hardcoded-color
 *     Hex (#abc, #aabbcc, ...) and rgb()/rgba() colors may only appear in
 *     src/theme.css, where tokens are defined. Everything else references
 *     var(--token).
 *
 *   no-bare-z-index
 *     z-index must reference a --z-* scale token (var(--z-modal) etc.),
 *     never a bare integer. Applies to every file, including theme.css.
 *
 *   no-outline-none-without-focus-visible
 *     A file that removes outlines (`outline: none`) must define a
 *     :focus-visible counterpart in the same file. File-level rule.
 *
 *   no-physical-box-props
 *     Physical box properties (margin-left, padding-right, left:/right:
 *     insets, border-left/right, corner radii, text-align: left/right,
 *     float/clear) where a logical equivalent exists, so a future RTL pass
 *     is not blocked. New/refactored CSS must use logical properties
 *     (margin-inline-start, inset-inline-end, ...).
 *
 * Grandfathering: violations listed in scripts/lint-css-exceptions.json are
 * tolerated; anything else fails. Exceptions are matched by normalized
 * declaration text (whitespace-collapsed, lowercased), so they survive line
 * shifts but break when the declaration is edited — which is the burn-down
 * workflow: edit a declaration, remove its exception. Stale exceptions
 * (listed but no longer present) also fail, keeping the list self-pruning.
 *
 * Usage:
 *   npm run lint:css              # report violations, exit 1 on any
 *   node scripts/lint-css.mjs --write-exceptions  # regenerate the
 *                                 # exceptions file from current violations
 *                                 # (grandfather everything; use only to
 *                                 # bootstrap or after mass renames)
 */

import { readFileSync, writeFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const SRC_DIR = fileURLToPath(new URL('../src', import.meta.url));
const EXCEPTIONS_PATH = fileURLToPath(
  new URL('./lint-css-exceptions.json', import.meta.url),
);
/** Only this file may define color tokens (hex/rgb/rgba literals). */
const TOKEN_FILE = 'theme.css';

/* ---------- utilities ---------- */

function* walkCss(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) yield* walkCss(path);
    else if (entry.endsWith('.css')) yield path;
  }
}

/** Strip /* ... *\/ comments so commented-out code is not linted. */
function stripComments(text) {
  return text.replace(/\/\*[\s\S]*?\*\//g, '');
}

/** Normalize a declaration for exception matching. */
function normalize(text) {
  return text.replace(/\s+/g, ' ').trim().toLowerCase();
}

/**
 * Split a comment-stripped stylesheet into declarations:
 * [{ prop, value, snippet, line }]. Values may span lines; data: URIs are
 * pre-masked so their `;base64,` does not split declarations.
 */
function parseDeclarations(text) {
  const masked = text.replace(/url\("data:[^"]*"\)|url\('data:[^']*'\)/g, (m) =>
    m.replace(/;/g, '%3B'),
  );
  const decls = [];
  const re = /([a-zA-Z-]+)\s*:\s*([^;{}]+);/g;
  let match;
  while ((match = re.exec(masked)) !== null) {
    const line = masked.slice(0, match.index).split('\n').length;
    decls.push({
      prop: match[1].toLowerCase(),
      value: match[2].trim(),
      snippet: normalize(`${match[1]}: ${match[2]};`),
      line,
    });
  }
  return decls;
}

/* ---------- rules ---------- */

const COLOR_RE = /#[0-9a-fA-F]{3,8}\b|\brgba?\(/;
const ZINDEX_RE = /^z-index$/;
const BARE_INT_RE = /^-?\d+$/;

const PHYSICAL_PROPS = new Set([
  'margin-left',
  'margin-right',
  'padding-left',
  'padding-right',
  'left',
  'right',
  'border-left',
  'border-left-color',
  'border-left-style',
  'border-left-width',
  'border-right',
  'border-right-color',
  'border-right-style',
  'border-right-width',
  'border-top-left-radius',
  'border-top-right-radius',
  'border-bottom-left-radius',
  'border-bottom-right-radius',
]);

const PHYSICAL_VALUE_RE = /^(text-align|float|clear)$/;

const RULES = [
  {
    id: 'no-hardcoded-color',
    level: 'declaration',
    check(file, decls) {
      if (file === TOKEN_FILE) return [];
      return decls
        .filter((d) => COLOR_RE.test(d.value))
        .map((d) => ({ snippet: d.snippet, line: d.line }));
    },
  },
  {
    id: 'no-bare-z-index',
    level: 'declaration',
    check(_file, decls) {
      return decls
        .filter((d) => ZINDEX_RE.test(d.prop) && BARE_INT_RE.test(d.value))
        .map((d) => ({ snippet: d.snippet, line: d.line }));
    },
  },
  {
    id: 'no-outline-none-without-focus-visible',
    level: 'file',
    check(_file, _decls, raw) {
      const hasOutlineNone = /outline\s*:\s*none\b/.test(raw);
      const hasFocusVisible = /:focus-visible/.test(raw);
      return hasOutlineNone && !hasFocusVisible
        ? [{ snippet: '*', line: 1 }]
        : [];
    },
  },
  {
    id: 'no-physical-box-props',
    level: 'declaration',
    check(_file, decls) {
      return decls
        .filter(
          (d) =>
            PHYSICAL_PROPS.has(d.prop) ||
            (PHYSICAL_VALUE_RE.test(d.prop) &&
              /^(left|right)$/.test(d.value)),
        )
        .map((d) => ({ snippet: d.snippet, line: d.line }));
    },
  },
];

/* ---------- runner ---------- */

function collectViolations() {
  // violations[ruleId][file] = [{ snippet, line }]
  const violations = {};
  for (const rule of RULES) violations[rule.id] = {};
  for (const path of walkCss(SRC_DIR)) {
    const file = relative(SRC_DIR, path);
    const raw = readFileSync(path, 'utf8');
    const text = stripComments(raw);
    const decls = parseDeclarations(text);
    for (const rule of RULES) {
      const found = rule.check(file, decls, text);
      if (found.length > 0) violations[rule.id][file] = found;
    }
  }
  return violations;
}

function loadExceptions() {
  try {
    return JSON.parse(readFileSync(EXCEPTIONS_PATH, 'utf8'));
  } catch {
    return {};
  }
}

function main() {
  const writeExceptions = process.argv.includes('--write-exceptions');
  const violations = collectViolations();

  if (writeExceptions) {
    const exceptions = {};
    for (const rule of RULES) {
      const byFile = violations[rule.id];
      const files = Object.keys(byFile).sort();
      if (files.length === 0) continue;
      exceptions[rule.id] = {};
      for (const file of files) {
        const snippets = [...new Set(byFile[file].map((v) => v.snippet))].sort();
        exceptions[rule.id][file] = snippets;
      }
    }
    writeFileSync(EXCEPTIONS_PATH, JSON.stringify(exceptions, null, 2) + '\n');
    const total = Object.values(exceptions).reduce(
      (n, byFile) =>
        n + Object.values(byFile).reduce((m, list) => m + list.length, 0),
      0,
    );
    console.log(
      `lint:css: wrote ${total} exceptions to scripts/lint-css-exceptions.json`,
    );
    return;
  }

  const exceptions = loadExceptions();
  let failures = 0;
  const matched = new Set(); // "ruleId\0file\0snippet"

  for (const rule of RULES) {
    const byFile = violations[rule.id];
    for (const [file, found] of Object.entries(byFile)) {
      const excepted = new Set(exceptions[rule.id]?.[file] ?? []);
      for (const v of found) {
        if (excepted.has(v.snippet)) {
          matched.add(`${rule.id}\0${file}\0${v.snippet}`);
        } else {
          failures++;
          console.error(
            `lint:css[${rule.id}] src/${file}:${v.line}  ${v.snippet}`,
          );
        }
      }
    }
  }

  // Stale exceptions fail too: the list is the burn-down tracker.
  for (const [ruleId, byFile] of Object.entries(exceptions)) {
    for (const [file, snippets] of Object.entries(byFile)) {
      for (const snippet of snippets) {
        if (!matched.has(`${ruleId}\0${file}\0${snippet}`)) {
          failures++;
          console.error(
            `lint:css[stale-exception] ${ruleId} src/${file}  ${snippet}`,
          );
        }
      }
    }
  }

  if (failures > 0) {
    console.error(`lint:css: ${failures} problem(s)`);
    process.exit(1);
  }
  console.log('lint:css: clean');
}

main();
