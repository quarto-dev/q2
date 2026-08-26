/**
 * axe-core accessibility baselines for the hub-client UI/UX modernization
 * (Phase 0). Scans the dev-harness key surfaces in both themes BEFORE any
 * token/consistency work lands, so a token migration that breaks contrast
 * (or any later a11y regression) fails immediately rather than phases later.
 *
 * Characterization model: the CURRENT serious/critical violations are
 * recorded in helpers/axe-baseline.json (per page+theme, rule → node
 * count). A scan fails when:
 *   - a serious/critical violation appears that the baseline does not know
 *     (a NEW regression), or
 *   - a baselined rule's node count GROWS (a worsened regression), or
 *   - a baselined rule disappears or shrinks (a FIX — the baseline must be
 *     regenerated to burn it down; keeps the file self-pruning).
 *
 * Regenerate after intentional fixes/changes (single worker — the write
 * happens in afterAll and must see every scan's result):
 *   AXE_BASELINE_WRITE=1 npx playwright test --config playwright.visual.config.ts baseline-a11y --workers=1
 *
 * Checker choice matches the quarto-cli ecosystem standard (bundled
 * axe-core, per the `axe` document-accessibility feature). Real-app key
 * screens (editor shell) are covered by the e2e accessibility suite;
 * extending scans to every gallery page is Phase 2; CI wiring is Phase 7.
 */

import { test, expect } from '@playwright/test';
import { AxeBuilder } from '@axe-core/playwright';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { THEMES, bootHarness } from './helpers/visual';

// bootHarness does two page loads (identity pinning) against a shared dev
// server; under full parallelism the default 30s budget is too tight.
test.setTimeout(60_000);

const BASELINE_PATH = fileURLToPath(
  new URL('./helpers/axe-baseline.json', import.meta.url),
);

const SCAN_PAGES: { page: string; label: string; selector: string }[] = [
  { page: 'projects-home', label: 'projects-home', selector: '.projects-home' },
  { page: 'dialog-new-file', label: 'dialog-new-file', selector: '.new-file-dialog' },
  { page: 'dialog-share', label: 'dialog-share', selector: '.share-dialog' },
  { page: 'dialog-new-asset', label: 'dialog-new-asset', selector: '.new-asset-dialog' },
  { page: 'sidebar', label: 'sidebar-sections', selector: '.sidebar-sections' },
  { page: 'about-tab', label: 'about-tab', selector: '.about-tab' },
  { page: 'header', label: 'minimal-header', selector: '.minimal-header' },
  { page: 'notifications', label: 'notifications', selector: '.ephemeral-session-banner' },
  { page: 'setup-migration', label: 'setup-migration', selector: '.setup-modal' },
  { page: 'setup-fresh', label: 'setup-fresh', selector: '.setup-modal' },
  { page: 'tokens', label: 'tokens', selector: 'text=Design tokens' },
  { page: 'gallery', label: 'gallery', selector: 'text=Component gallery' },
];

/** key → { ruleId: nodeCount } */
type AxeBaseline = Record<string, Record<string, number>>;

function loadBaseline(): AxeBaseline {
  try {
    return JSON.parse(readFileSync(BASELINE_PATH, 'utf8'));
  } catch {
    return {};
  }
}

const WRITE_MODE = process.env.AXE_BASELINE_WRITE === '1';
const written: AxeBaseline = {};

for (const { page, label, selector } of SCAN_PAGES) {
  for (const theme of THEMES) {
    test(`axe: ${label} — ${theme} theme`, async ({ page: browserPage }) => {
      await bootHarness(browserPage, page, selector, theme);

      const results = await new AxeBuilder({ page: browserPage }).analyze();
      const blocking = results.violations.filter(
        (v) => v.impact === 'serious' || v.impact === 'critical',
      );
      const counts: Record<string, number> = {};
      for (const v of blocking) counts[v.id] = v.nodes.length;

      const key = `${label}|${theme}`;
      if (WRITE_MODE) {
        if (Object.keys(counts).length > 0) written[key] = counts;
        return;
      }

      const baseline = loadBaseline();
      const expected = baseline[key] ?? {};
      const problems: string[] = [];
      for (const [ruleId, count] of Object.entries(counts)) {
        if (!(ruleId in expected)) {
          problems.push(`NEW violation: ${ruleId} (${count} node(s))`);
        } else if (count > expected[ruleId]) {
          problems.push(
            `WORSENED: ${ruleId} grew from ${expected[ruleId]} to ${count} node(s)`,
          );
        } else if (count < expected[ruleId]) {
          problems.push(
            `IMPROVED: ${ruleId} shrank from ${expected[ruleId]} to ${count} node(s) — regenerate the baseline (AXE_BASELINE_WRITE=1)`,
          );
        }
      }
      for (const ruleId of Object.keys(expected)) {
        if (!(ruleId in counts)) {
          problems.push(
            `FIXED: ${ruleId} no longer fires — regenerate the baseline (AXE_BASELINE_WRITE=1)`,
          );
        }
      }
      if (problems.length > 0) {
        console.log(
          `axe baseline drift (${key}):\n` +
            problems.map((p) => `  ${p}`).join('\n') +
            '\n  current: ' +
            JSON.stringify(counts),
        );
      }
      expect(problems).toEqual([]);
    });
  }
}

// After all scans in write mode, persist the baseline.
test.afterAll(() => {
  if (!WRITE_MODE) return;
  const sorted: AxeBaseline = {};
  for (const key of Object.keys(written).sort()) sorted[key] = written[key];
  writeFileSync(BASELINE_PATH, JSON.stringify(sorted, null, 2) + '\n');
  console.log(
    `axe baseline: wrote ${Object.keys(sorted).length} entries to helpers/axe-baseline.json`,
  );
});
