/**
 * bd-abo9m23f — a mouse drag inside a single block must open the rich-text
 * editor with the dragged selection recreated (not a bare caret at the
 * release point), and a drag spanning blocks must not open an editor at all
 * (it would destroy a selection the user may only want to copy).
 *
 * Real-binary e2e (drives target/debug/q2 via startPreviewServer) with REAL
 * trusted mouse drags (`mouse.down/move/up`) — the only place the browser's
 * own drag→selection→pointerup pipeline and posAtCoords geometry are
 * exercised; unit tests mock both (jsdom has no layout). The invariant
 * asserted is exactly the feature's contract: the editor's opening selection
 * equals the native selection that existed at `pointerup`.
 *
 * Build chain prerequisite (the binary does NOT auto-rebuild the embedded SPA):
 *   cargo xtask build-q2-preview-spa
 *   cargo build -p quarto --bin q2
 */

import { test, expect, type Page } from '@playwright/test';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

let server: PreviewServerHandle;

const FIXTURE = [
    '---',
    'title: Drag selection e2e',
    '---',
    '',
    'The quick brown fox jumps over the lazy dog in the first paragraph.',
    '',
    'A second paragraph sits below with more words to select across.',
    '',
].join('\n');

/** Page-viewport coordinates of a word edge inside a preview block (the
 *  preview renders in an iframe, so the iframe offset is added). `inset`
 *  nudges the point INTO the word so the drag press lands on a glyph, not on
 *  the boundary between two characters. */
async function wordPoint(
    page: Page,
    blockIndex: number,
    word: string,
    edge: 'start' | 'end',
): Promise<{ x: number; y: number }> {
    const iframeBox = await page.locator('iframe').boundingBox();
    if (!iframeBox) throw new Error('preview iframe has no bounding box');
    const rel = await page
        .frameLocator('iframe')
        .locator('p[data-block-pool-id]')
        .nth(blockIndex)
        .evaluate((el, args: { word: string; edge: 'start' | 'end' }) => {
            const doc = el.ownerDocument;
            const walker = doc.createTreeWalker(el, NodeFilter.SHOW_TEXT);
            while (walker.nextNode()) {
                const n = walker.currentNode as Text;
                const idx = (n.textContent ?? '').indexOf(args.word);
                if (idx < 0) continue;
                const r = doc.createRange();
                r.setStart(n, args.edge === 'start' ? idx : idx + args.word.length);
                r.collapse(true);
                const rect = r.getClientRects()[0];
                if (!rect) throw new Error(`no caret rect for "${args.word}"`);
                const inset = args.edge === 'start' ? 1 : -1;
                return { x: rect.left + inset, y: rect.top + rect.height / 2 };
            }
            throw new Error(`word not found in block: "${args.word}"`);
        }, { word, edge });
    return { x: iframeBox.x + rel.x, y: iframeBox.y + rel.y };
}

/** Record, in the iframe, the native selection text as it stands when
 *  `pointerup` fires (capture phase — before React's activation handler). */
async function armPointerupSelectionRecorder(page: Page): Promise<void> {
    await page.frameLocator('iframe').locator('body').evaluate((body) => {
        const win = body.ownerDocument.defaultView as Window & {
            __selAtPointerup?: string | null;
        };
        win.__selAtPointerup = null;
        win.addEventListener(
            'pointerup',
            () => {
                win.__selAtPointerup =
                    win.getSelection()?.toString() ?? null;
            },
            true,
        );
    });
}

/** Snapshot the iframe's editor/selection state. */
async function editorState(page: Page) {
    return page.frameLocator('iframe').locator('body').evaluate((body) => {
        const win = body.ownerDocument.defaultView as Window & {
            __selAtPointerup?: string | null;
        };
        const sel = win.getSelection();
        const pm = body.querySelector('.ProseMirror');
        let backward = false;
        if (sel && !sel.isCollapsed && sel.anchorNode && sel.focusNode) {
            backward =
                sel.anchorNode === sel.focusNode
                    ? sel.focusOffset < sel.anchorOffset
                    : !!(
                          sel.anchorNode.compareDocumentPosition(sel.focusNode) &
                          Node.DOCUMENT_POSITION_PRECEDING
                      );
        }
        return {
            recordedAtPointerup: win.__selAtPointerup ?? null,
            editorOpen: pm != null,
            textareaOpen: body.querySelector('textarea') != null,
            selText: sel?.toString() ?? '',
            collapsed: sel?.isCollapsed ?? true,
            backward,
            selInEditor:
                pm != null && sel?.anchorNode != null
                    ? pm.contains(sel.anchorNode)
                    : false,
        };
    });
}

async function dragOpen(page: Page): Promise<void> {
    server = await startPreviewServer({
        allowEdit: true,
        fixtureFiles: [{ path: 'index.qmd', content: FIXTURE }],
    });
    await page.goto(server.url);
    await page.waitForFunction(() => {
        const inner = document.querySelector('iframe')?.contentDocument;
        return inner?.querySelector('p[data-block-pool-id]') != null;
    }, null, { timeout: 30_000 });
    await armPointerupSelectionRecorder(page);
}

async function drag(page: Page, from: { x: number; y: number }, to: { x: number; y: number }) {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(to.x, to.y, { steps: 12 });
    await page.mouse.up();
}

test.describe('bd-abo9m23f — drag selection carries into the rich-text editor', () => {
    test.setTimeout(120_000);

    test.afterEach(async () => {
        await server?.stop();
    });

    test('forward drag within one paragraph opens the editor with the dragged selection', async ({ page }) => {
        await dragOpen(page);

        const from = await wordPoint(page, 0, 'quick', 'start');
        const to = await wordPoint(page, 0, 'lazy', 'end');
        await drag(page, from, to);

        await page.frameLocator('iframe').locator('.ProseMirror').waitFor({ timeout: 10_000 });
        // Selection placement runs one rAF after mount — poll rather than race it.
        await expect
            .poll(async () => (await editorState(page)).collapsed, {
                message: 'editor selection should become non-collapsed',
                timeout: 5_000,
            })
            .toBe(false);

        const state = await editorState(page);
        expect(state.editorOpen).toBe(true);
        // The contract: what the user had selected at pointerup is what the
        // editor opens with.
        expect(state.recordedAtPointerup).toBeTruthy();
        expect(state.selText).toBe(state.recordedAtPointerup);
        expect(state.selText).toContain('brown fox jumps');
        expect(state.selInEditor).toBe(true);
        expect(state.backward).toBe(false);
    });

    test('backward drag preserves selection direction (head at the release end)', async ({ page }) => {
        await dragOpen(page);

        // Drag right-to-left: press at the end of "lazy", release at "quick".
        const from = await wordPoint(page, 0, 'lazy', 'end');
        const to = await wordPoint(page, 0, 'quick', 'start');
        await drag(page, from, to);

        await page.frameLocator('iframe').locator('.ProseMirror').waitFor({ timeout: 10_000 });
        await expect
            .poll(async () => (await editorState(page)).collapsed, {
                message: 'editor selection should become non-collapsed',
                timeout: 5_000,
            })
            .toBe(false);

        const state = await editorState(page);
        expect(state.selText).toBe(state.recordedAtPointerup);
        expect(state.selText).toContain('brown fox jumps');
        expect(state.selInEditor).toBe(true);
        expect(state.backward, 'a right-to-left drag must stay backward in the editor').toBe(true);
    });

    test('cross-block drag opens no editor and leaves the selection intact', async ({ page }) => {
        await dragOpen(page);

        const from = await wordPoint(page, 0, 'jumps', 'start');
        const to = await wordPoint(page, 1, 'below', 'end');
        await drag(page, from, to);

        // Nothing should open; give the (absent) activation a beat to misfire.
        await page.waitForTimeout(800);

        const state = await editorState(page);
        expect(state.editorOpen, 'cross-block drag must not open the rich-text editor').toBe(false);
        expect(state.textareaOpen, 'cross-block drag must not open the plain editor').toBe(false);
        // The user's selection survives (this is what enables copy).
        expect(state.collapsed).toBe(false);
        expect(state.selText).toContain('lazy dog in the first paragraph.');
        expect(state.selText).toContain('A second paragraph');
    });
});
