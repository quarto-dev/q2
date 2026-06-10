import { expect, type Page } from '@playwright/test';

/**
 * Poll the Automerge VFS until the file's content satisfies all checks.
 *
 * Uses `window.__quartoTest.wasmRenderer.getFileContent()` which reads
 * directly from the Automerge-backed VFS layer — the right layer to
 * prove a QMD actually changed, not just the DOM.
 */
export async function assertAutomerge(
    page: Page,
    filename: string,
    { contains = [], lacks = [] }: { contains?: string[]; lacks?: string[] },
): Promise<void> {
    await expect(async () => {
        const text = await page.evaluate(async (f) => {
            await (window as any).__quartoTestReady;
            return (window as any).__quartoTest?.wasmRenderer.getFileContent(f) as string | null;
        }, filename);
        expect(text, `getFileContent(${filename}) must return a string`).not.toBeNull();
        for (const s of contains) expect(text, `QMD must contain "${s}"`).toContain(s);
        for (const s of lacks) expect(text, `QMD must not contain "${s}"`).not.toContain(s);
    }).toPass({ timeout: 15_000 });
}
