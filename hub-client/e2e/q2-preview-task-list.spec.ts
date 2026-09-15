/**
 * q2-preview task-list layout (bd-qif9l4cx).
 *
 * Reported 2026-09-10 on quarto-hub.com: a `- [x] item` rendered its
 * checkbox alone on one line with the item text on the next. The cause was
 * DOM shape, not CSS — the `<li>` built the `<label>` around
 * `<Node node={Plain}>`, and CommentBlock's positioned `<div>` wrapper (a
 * block box) landed inside the inline label after the `<input>`. The
 * preview-renderer integration tests now assert the label holds no block
 * descendants, but jsdom has no layout: only a real browser can prove the
 * checkbox and its text share a line box. This spec is that proof, on the
 * surface the bug was reported on (hub-client's q2-preview iframe).
 *
 * One document carries the three list shapes the fix covers:
 *   - the reporter's nested list mixing plain items with one `[x]` item;
 *   - a tight all-task list (`<ul class="task-list">`);
 *   - a loose all-task list (Para-leading items, `li > p > label`).
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/q2-preview-task-list.spec.ts \
 *     --project=chromium --workers=1
 */

import { test, expect, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import {
    bootstrapProjectSet,
    createProjectOnServer,
    seedProjectInBrowser,
    getServerUrl,
} from './helpers/projectFactory';
import { waitForPreviewRender } from './helpers/previewExtraction';

const Q2_IFRAME = 'iframe[src*="q2-preview.html"]';

const DOC = [
    '---',
    'title: Task lists',
    'format: q2-preview',
    '---',
    '',
    '## Nested',
    '',
    '* \\[Elliot\\]',
    '  * looking into QH file sync bug a bit',
    '  * part-way thru getting sandboxed-preview up to feature parity with q2-preview',
    '    - once it is we should (consider) replace q2-preview with it',
    '  * [x] working with Julia on getting her work on replacing vdocs in Positron merged',
    '',
    '## Tight',
    '',
    '- [ ] tight todo',
    '- [x] tight done',
    '',
    '## Loose',
    '',
    '- [ ] loose todo',
    '',
    '- [x] loose done',
    '',
].join('\n');

/** Per-checkbox geometry + DOM facts, measured inside the iframe. */
interface CheckboxFacts {
    text: string;
    checked: boolean;
    /** Tag chain from the label up to (excluding) the <li>. */
    ancestors: string[];
    ulClass: string | null;
    blockInsideLabel: boolean;
    /** Vertical distance between the input's top and the top of the first
     * client rect of the text that follows it (both viewport-relative). */
    textOffsetY: number;
    inputHeight: number;
}

async function openDoc(page: Page): Promise<void> {
    const serverUrl = getServerUrl();
    const docId = await createProjectOnServer(serverUrl, [
        { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
        { path: 'doc.qmd', content: DOC, contentType: 'text' },
    ]);
    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, docId, serverUrl);
    await page.goto(`/#/p/${localId}/file/doc.qmd`);
    await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30000 });
}

async function collectCheckboxFacts(page: Page): Promise<CheckboxFacts[]> {
    const frame = page.frameLocator(Q2_IFRAME);
    const inputs = frame.locator('input[type="checkbox"]');
    await expect(inputs).toHaveCount(5, { timeout: 30000 });
    return inputs.evaluateAll((els) =>
        els.map((el) => {
            const input = el as HTMLInputElement;
            const label = input.parentElement as HTMLElement;
            const li = input.closest('li') as HTMLElement;
            const ancestors: string[] = [];
            for (let e = label; e && e !== li; e = e.parentElement as HTMLElement) {
                ancestors.push(e.tagName.toLowerCase());
            }
            // First rect of the text node right after the input.
            const textNode = Array.from(label.childNodes).find(
                (n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? '').trim() !== '',
            );
            const range = document.createRange();
            range.selectNodeContents(textNode ?? label);
            const textRect = range.getClientRects()[0] ?? range.getBoundingClientRect();
            const inputRect = input.getBoundingClientRect();
            return {
                text: label.textContent ?? '',
                checked: input.checked,
                ancestors,
                ulClass: input.closest('ul')?.getAttribute('class') ?? null,
                blockInsideLabel: label.querySelector('div, p, ul, ol') !== null,
                textOffsetY: Math.abs(textRect.top - inputRect.top),
                inputHeight: inputRect.height,
            };
        }),
    );
}

test.describe('q2-preview task lists', () => {
    test('checkbox and item text share one line for nested, tight and loose items', async ({
        page,
    }) => {
        await openDoc(page);
        const facts = await collectCheckboxFacts(page);

        expect(facts.map((f) => f.text)).toEqual([
            'working with Julia on getting her work on replacing vdocs in Positron merged',
            'tight todo',
            'tight done',
            'loose todo',
            'loose done',
        ]);
        expect(facts.map((f) => f.checked)).toEqual([true, false, true, false, true]);

        for (const f of facts) {
            // The regression: a block box inside the label pushed the text
            // onto its own line. Now every wrapper is an ancestor of the
            // label, and the text's first rect starts within the checkbox's
            // own height of the checkbox (same line box).
            expect(f.blockInsideLabel, `${f.text}: block inside label`).toBe(false);
            expect(f.ancestors[0], `${f.text}: input parent`).toBe('label');
            // (The label's own height is deliberately not asserted: a long
            // item may wrap in a narrow preview pane, which is fine.)
            expect(f.textOffsetY, `${f.text}: text/checkbox vertical offset`).toBeLessThan(
                f.inputHeight,
            );
        }

        // Writer parity: `class="task-list"` iff every item is a task item.
        expect(facts[0].ulClass).toBeNull();
        expect(facts[1].ulClass).toBe('task-list');
        expect(facts[3].ulClass).toBe('task-list');
        // Loose items keep the writer's `li > p > label` shape.
        expect(facts[3].ancestors).toContain('p');
        expect(facts[4].ancestors).toContain('p');
        expect(facts[1].ancestors).not.toContain('p');
    });
});
