// @vitest-environment jsdom
/**
 * Preview <-> render DOM parity runner (fourth smoke-all runner).
 *
 * For every smoke-all fixture whose `_quarto.tests.html` carries
 * `dom-parity: true`, render it twice through the same WASM module:
 *   - `render_page_in_project`   -> native HTML writer -> full page HTML
 *   - `render_page_for_preview`  -> the same Rust function with
 *                                   `prefer_preview_format: true`: pipeline
 *                                   stopped before `render-html-body`
 *                                   -> Pandoc AST JSON
 * Mount the AST read-only with the real q2-preview React registry under
 * jsdom, and require the canonical form of `main#quarto-document-content`
 * to be identical on both sides. Normalisation rules and their reasons
 * live in `ts-packages/preview-renderer/src/test-utils/domParity.ts`.
 *
 * Opt-in is curated: an opted-in fixture that diverges FAILS. There is
 * no expected-failure list — fix the divergence or remove the opt-in.
 *
 * Plan: claude-notes/plans/2026-08-24-preview-render-dom-parity-harness.md
 * Manual predecessor: .claude/skills/preview-render-parity/SKILL.md
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile, mkdir, writeFile } from 'fs/promises';
import { join, relative, resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import { JSDOM } from 'jsdom';
import { render, cleanup } from '@testing-library/react';
import { Ast } from '@quarto/preview-renderer/framework';
import { previewRegistry } from '@quarto/preview-renderer';
import {
  extractParityRoot,
  compareParity,
  ParityRuleViolation,
} from '@quarto/preview-renderer/test-utils/domParity';
import {
  SMOKE_ALL_DIR,
  loadSmokeWasm,
  discoverTestFiles,
  readFrontmatter,
  readTestsBlock,
  shouldSkip,
  populateVfs,
  buildUserGrammarsHandle,
  type WasmModule,
} from '../test-utils/smokeAllFixtures';

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = resolve(__dirname, '../../test-results/parity');

let wasm: WasmModule;

beforeAll(async () => {
  wasm = await loadSmokeWasm();
});

beforeEach(() => {
  wasm.vfs_clear();
  cleanup();
});

interface Sides {
  renderMain: Element;
  previewMain: Element;
}

/** Render one fixture both ways and return the two parity roots. */
async function renderBothSides(testFile: string): Promise<Sides> {
  const { vfsPath, projectFiles } = await populateVfs(wasm, testFile);
  const grammars = await buildUserGrammarsHandle(wasm, projectFiles);

  const renderRes = JSON.parse(await wasm.render_page_in_project(vfsPath, grammars)) as {
    success: boolean; html?: string; error?: string;
  };
  if (!renderRes.success || !renderRes.html) {
    throw new Error(`render_page_in_project failed: ${renderRes.error ?? 'no html'}`);
  }

  const previewRes = JSON.parse(
    await wasm.render_page_for_preview(vfsPath, grammars, undefined),
  ) as { success: boolean; ast_json?: string; error?: string };
  if (!previewRes.success || !previewRes.ast_json) {
    throw new Error(`render_page_for_preview failed: ${previewRes.error ?? 'no ast_json'}`);
  }

  const renderDoc = new JSDOM(renderRes.html).window.document;
  // Read-only mount: no PreviewContext / AssetManifestContext /
  // IncrementalContext on purpose (plan § Global Constraints).
  const { container } = render(
    <Ast
      astJson={previewRes.ast_json}
      currentFilePath={vfsPath}
      onNavigateToDocument={() => {}}
      setAst={() => {}}
      registry={previewRegistry}
    />,
  );
  return {
    renderMain: extractParityRoot(renderDoc, 'render'),
    previewMain: extractParityRoot(container, 'preview'),
  };
}

async function writeArtifacts(relPath: string, renderText: string, previewText: string) {
  const dir = join(OUT_DIR, relPath.replace(/[\\/]/g, '__'));
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, 'render.norm.txt'), renderText);
  await writeFile(join(dir, 'preview.norm.txt'), previewText);
  return dir;
}

/**
 * Produce vitest's own diff text for two strings without throwing. Vitest
 * doesn't expose its differ as a standalone function, so this harvests it by
 * catching `expect` — the thrown assertion error's message is the same
 * pretty diff the reporter prints.
 */
function diffText(expected: string, actual: string): string {
  try {
    expect(actual).toBe(expected);
    return '';
  } catch (e) {
    return (e as Error).message;
  }
}

async function optedInFixtures(): Promise<string[]> {
  const files = await discoverTestFiles(SMOKE_ALL_DIR);
  const out: string[] = [];
  for (const f of files) {
    const block = readTestsBlock(readFrontmatter(await readFile(f, 'utf-8')));
    if (!block || shouldSkip(block.run)) continue;
    if (block.formats['html']?.['dom-parity'] === true) out.push(f);
  }
  return out;
}

describe('smoke-all preview <-> render DOM parity', () => {
  // Same single-`it` shape as smokeAll.wasm.test.ts: vitest collects tests
  // synchronously, and discovery is async.
  it('every opted-in fixture has identical <main> DOM on both sides', async () => {
    const fixtures = await optedInFixtures();
    const smokeFilter = process.env.SMOKE_FILTER || '';
    const failures: string[] = [];
    let compared = 0;

    for (const testFile of fixtures) {
      const relPath = relative(SMOKE_ALL_DIR, testFile);
      if (smokeFilter && !relPath.includes(smokeFilter)) continue;
      wasm.vfs_clear();
      cleanup();
      const started = performance.now();
      try {
        const { renderMain, previewMain } = await renderBothSides(testFile);
        const result = compareParity(renderMain, previewMain);
        compared++;
        if (!result.equal) {
          const dir = await writeArtifacts(relPath, result.render, result.preview);
          failures.push(
            `${relPath} [html]: parity mismatch (artifacts: ${dir})\n${diffText(result.render, result.preview)}`,
          );
        }
      } catch (e) {
        const kind = e instanceof ParityRuleViolation ? 'rule violation' : 'error';
        failures.push(`${relPath} [html]: ${kind}: ${(e as Error).message}`);
      }
      console.log(`  parity ${relPath}: ${Math.round(performance.now() - started)} ms`);
    }

    console.log(`\nParity results: ${compared} compared, ${failures.length} failed, ${fixtures.length} opted in`);
    expect(fixtures.length, 'at least one fixture must opt in (dom-parity: true)').toBeGreaterThan(0);
    expect(failures, `${failures.length} parity failure(s):\n${failures.join('\n\n')}`).toHaveLength(0);
  });

  it('reports an injected divergence (harness self-check)', async () => {
    const [first] = await optedInFixtures();
    expect(first, 'needs one opted-in fixture').toBeDefined();
    const { renderMain, previewMain } = await renderBothSides(first);
    expect(compareParity(renderMain, previewMain).equal).toBe(true);

    // Inject: add a class to the first element on the preview side.
    const victim = previewMain.querySelector('p, h1, h2, pre, div, section');
    expect(victim).not.toBeNull();
    victim!.classList.add('injected-divergence');

    const result = compareParity(renderMain, previewMain);
    expect(result.equal).toBe(false);
    expect(result.preview).toContain('injected-divergence');
    expect(result.render).not.toContain('injected-divergence');
  });
});
