/**
 * Smoke-all assertion functions for Playwright E2E tests.
 *
 * Ported from hub-client/src/services/smokeAll.wasm.test.ts.
 * Adapted to use Playwright's page/frame APIs instead of direct WASM calls.
 */

import { expect, type Page } from '@playwright/test';
import type { AssertionSpec } from './smokeAllDiscovery';
import {
  getPreviewHtml,
  getPreviewCss,
  getRenderDiagnostics,
  type RenderDiagnostic,
} from './previewExtraction';

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
): Promise<void> {
  // Get diagnostics once for all assertion types that need them
  const diag = await getRenderDiagnostics(page, documentPath);
  const allMsgs = collectMessages(diag.diagnostics, diag.warnings);

  for (const spec of assertions) {
    switch (spec.type) {
      case 'ensureFileRegexMatches': {
        expect(diag.success, `Render failed: ${diag.error}`).toBe(true);
        const rawHtml = await getPreviewHtml(page, documentPath);
        const html = stripSourceTrackingSpans(rawHtml);
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
        expect(diag.success, `Render failed: ${diag.error}`).toBe(true);
        const previewFrame = page.frameLocator('iframe.preview-active');
        for (const selector of spec.selectors) {
          await expect(
            previewFrame.locator(selector).first(),
            `ensureHtmlElements: expected selector "${selector}" to match`,
          ).toBeAttached();
        }
        for (const selector of spec.noMatchSelectors) {
          await expect(
            previewFrame.locator(selector),
            `ensureHtmlElements: expected selector "${selector}" NOT to match`,
          ).toHaveCount(0);
        }
        break;
      }

      case 'ensureCssRegexMatches': {
        expect(diag.success, `Render failed: ${diag.error}`).toBe(true);
        const css = await getPreviewCss(page);
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
          diag.success,
          `noErrors: render failed: ${diag.error}${errors.length ? '\n  Diagnostics: ' + errors.map((e) => e.message).join(', ') : ''}`,
        ).toBe(true);
        break;
      }

      case 'noErrorsOrWarnings': {
        const errors = allMsgs.filter((m) => m.level === 'ERROR');
        expect(
          diag.success,
          `noErrorsOrWarnings: render failed: ${diag.error}${errors.length ? '\n  Diagnostics: ' + errors.map((e) => e.message).join(', ') : ''}`,
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
          diag.success,
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
