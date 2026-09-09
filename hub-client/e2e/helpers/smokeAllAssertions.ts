/**
 * Smoke-all assertion functions for Playwright E2E tests.
 *
 * Ported from hub-client/src/services/smokeAll.wasm.test.ts.
 * Adapted to use Playwright's page/frame APIs instead of direct WASM calls.
 */

import { expect, type Page } from '@playwright/test';
import type { AssertionSpec } from './smokeAllDiscovery';
import {
  getPreviewCss,
  renderForAssertions,
  previewIframeSelector,
  type PreviewIframeKind,
  type RenderDiagnostic,
} from './previewExtraction';

// Live-iframe element assertions (ensureHtmlElements) read the *running*
// Preview iframe, not a fresh WASM re-render. After late-arriving sibling
// files sync into the VFS, the Preview re-renders (20ms-debounced) and the
// iframe DOM catches up — but on a contended CI runner that re-render can
// take well over Playwright's 5s default expect timeout. Give these
// assertions real headroom so a slow-but-correct catch-up isn't reported as
// a failure. (Negative `toHaveCount(0)` checks return immediately when the
// element is correctly absent, so this only costs wall-clock on a genuine
// failure.)
const ELEMENT_WAIT_TIMEOUT = 30000;

// ---------------------------------------------------------------------------
// HTML normalization
// ---------------------------------------------------------------------------

/**
 * Strip source-tracking wrapper spans from rendered HTML.
 *
 * The WASM renderer wraps inline text in `<span data-sid="..." data-loc="...">`.
 * The smoke-all fixture patterns were written for output without these spans,
 * so we unwrap them before regex matching, keeping the text content.
 *
 * Example: `<span data-sid="5" data-loc="0:1:1-1:5">Hello</span>` → `Hello`
 */
function stripSourceTrackingSpans(html: string): string {
  // Strip spans with both data-sid and data-loc (paragraph-level tracking)
  // AND spans with only data-sid (inline text tracking added by source-location: full)
  return html.replace(/<span data-sid="[^"]*"(?: data-loc="[^"]*")?>([^<]*)<\/span>/g, '$1');
}

// ---------------------------------------------------------------------------
// Diagnostic helpers
// ---------------------------------------------------------------------------

function kindToLevel(kind: string): string {
  switch (kind.toLowerCase()) {
    case 'error':
      return 'ERROR';
    case 'warning':
      return 'WARN';
    case 'info':
      return 'INFO';
    case 'note':
      return 'DEBUG';
    default:
      return kind.toUpperCase();
  }
}

function collectMessages(
  diagnostics: RenderDiagnostic[],
  warnings: RenderDiagnostic[],
): { level: string; message: string }[] {
  const msgs: { level: string; message: string }[] = [];
  for (const d of diagnostics) {
    msgs.push({ level: kindToLevel(d.kind), message: d.title });
  }
  for (const w of warnings) {
    msgs.push({ level: kindToLevel(w.kind), message: w.title });
  }
  return msgs;
}

// ---------------------------------------------------------------------------
// Assertion runner
// ---------------------------------------------------------------------------

/**
 * Run all assertions for a smoke-all test against the live page.
 *
 * @param page - Playwright page with the project loaded and preview rendered
 * @param documentPath - Path of the rendered document (relative to project root)
 * @param assertions - Assertion specs parsed from frontmatter
 * @param expectsError - Whether the test expects a render failure
 */
export async function runAssertions(
  page: Page,
  documentPath: string,
  assertions: AssertionSpec[],
  expectsError: boolean,
  opts: {
    kind: PreviewIframeKind;
    /**
     * When set (to the owning parity strand id), `ensureHtmlElements`
     * assertions are skipped for this fixture — see
     * `DOM_ASSERTIONS_PENDING_PARITY` in smokeAllDiscovery.ts.
     */
    skipDomAssertionsFor?: string;
  },
): Promise<void> {
  const kind: PreviewIframeKind = opts.kind;
  const iframeSel = previewIframeSelector(kind);

  // q2-debug doesn't go through the WASM html render path, so skip the
  // render entirely (it would re-render as html and report unrelated noise).
  // For every other kind, render ONCE and reuse the result for both the HTML
  // pattern matching and the diagnostics — see renderForAssertions for why
  // the previous two-render approach amplified the slow-render flakiness.
  const render =
    kind === 'q2-debug'
      ? { success: true, error: undefined, html: '', diagnostics: [], warnings: [] }
      : await renderForAssertions(page, documentPath);
  const allMsgs = collectMessages(render.diagnostics, render.warnings);

  for (const spec of assertions) {
    switch (spec.type) {
      case 'ensureFileRegexMatches': {
        expect(render.success, `Render failed: ${render.error}`).toBe(true);
        const html = stripSourceTrackingSpans(render.html);
        for (const pattern of spec.matches) {
          expect(
            new RegExp(pattern, 'm').test(html),
            `ensureFileRegexMatches: expected pattern "${pattern}" to match in HTML`,
          ).toBe(true);
        }
        for (const pattern of spec.noMatches) {
          expect(
            new RegExp(pattern, 'm').test(html),
            `ensureFileRegexMatches: expected pattern "${pattern}" NOT to match in HTML`,
          ).toBe(false);
        }
        break;
      }

      case 'ensureHtmlElements': {
        expect(render.success, `Render failed: ${render.error}`).toBe(true);
        if (opts.skipDomAssertionsFor) {
          // Parsed by the offline analyzer alongside the other
          // `[smoke-diag]` lines, so skipped DOM coverage stays visible.
          console.log(
            `[smoke-diag] dom-assertions-skipped strand=${opts.skipDomAssertionsFor} kind=${kind}`,
          );
          break;
        }
        const previewFrame = page.frameLocator(iframeSel);
        for (const selector of spec.selectors) {
          await expect(
            previewFrame.locator(selector).first(),
            `ensureHtmlElements: expected selector "${selector}" to match`,
          ).toBeAttached({ timeout: ELEMENT_WAIT_TIMEOUT });
        }
        for (const selector of spec.noMatchSelectors) {
          await expect(
            previewFrame.locator(selector),
            `ensureHtmlElements: expected selector "${selector}" NOT to match`,
          ).toHaveCount(0, { timeout: ELEMENT_WAIT_TIMEOUT });
        }
        break;
      }

      case 'ensureCssRegexMatches': {
        expect(render.success, `Render failed: ${render.error}`).toBe(true);
        // Stylesheets come from the render string, not the live iframe, so
        // the assertion is independent of which renderer is mounted.
        const css = await getPreviewCss(page, render.html);
        expect(
          css.length,
          'ensureCssRegexMatches: no CSS content found',
        ).toBeGreaterThan(0);
        for (const pattern of spec.matches) {
          expect(
            new RegExp(pattern, 'm').test(css),
            `ensureCssRegexMatches: expected CSS pattern "${pattern}" to match`,
          ).toBe(true);
        }
        for (const pattern of spec.noMatches) {
          expect(
            new RegExp(pattern, 'm').test(css),
            `ensureCssRegexMatches: expected CSS pattern "${pattern}" NOT to match`,
          ).toBe(false);
        }
        break;
      }

      case 'noErrors': {
        const errors = allMsgs.filter((m) => m.level === 'ERROR');
        expect(
          render.success,
          `noErrors: render failed: ${render.error}${errors.length ? '\n  Diagnostics: ' + errors.map((e) => e.message).join(', ') : ''}`,
        ).toBe(true);
        break;
      }

      case 'noErrorsOrWarnings': {
        const errors = allMsgs.filter((m) => m.level === 'ERROR');
        expect(
          render.success,
          `noErrorsOrWarnings: render failed: ${render.error}${errors.length ? '\n  Diagnostics: ' + errors.map((e) => e.message).join(', ') : ''}`,
        ).toBe(true);
        const warnings = allMsgs.filter((m) => m.level === 'WARN');
        expect(
          warnings.length,
          `noErrorsOrWarnings: unexpected warnings: ${warnings.map((w) => w.message).join(', ')}`,
        ).toBe(0);
        break;
      }

      case 'shouldError': {
        expect(
          render.success,
          'shouldError: expected render to fail but it succeeded',
        ).toBe(false);
        break;
      }

      case 'printsMessage': {
        const filtered = allMsgs.filter((m) => m.level === spec.level);
        const re = new RegExp(spec.regex);
        const anyMatch = filtered.some((m) => re.test(m.message));

        if (spec.negate) {
          expect(
            anyMatch,
            `printsMessage: expected no ${spec.level} message matching /${spec.regex}/ but found one`,
          ).toBe(false);
        } else {
          expect(
            anyMatch,
            `printsMessage: expected a ${spec.level} message matching /${spec.regex}/ but none found among: [${filtered.map((m) => m.message).join(', ')}]`,
          ).toBe(true);
        }
        break;
      }
    }
  }
}
