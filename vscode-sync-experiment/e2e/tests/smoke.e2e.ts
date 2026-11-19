/**
 * Smoke tests (GE-8): a real VS Code with the extension, against the local
 * in-process test hub. The permanent subset is chosen in GE-14.
 *
 * Not watched by a human: these assert on disk contents, editor text, and the
 * hub's documents, never on a screenshot.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { expect, test } from '@playwright/test';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { HeadlessPeer, Seeder, ensureWasm, readText, startHub, tempDir, waitFor } from '../helpers/hub';
import { launchVscode, type RunningVscode } from '../helpers/launch';
import { activeEditorText, fillInputBox, openFile, runCommand } from '../helpers/palette';

let hub: TestHub;
let vscode: RunningVscode | undefined;
const cleanup: (() => Promise<void>)[] = [];

test.beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
test.afterAll(async () => {
  await hub.stop();
});
test.afterEach(async () => {
  await vscode?.close();
  vscode = undefined;
  for (const c of cleanup.splice(0).reverse()) await c().catch(() => {});
});

async function dir(prefix: string): Promise<string> {
  const d = await tempDir(prefix);
  cleanup.push(() => fs.rm(d, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }));
  return d;
}

async function waitForHubHeads(peer: HeadlessPeer): Promise<void> {
  for (const { docId, heads } of peer.inventory()) {
    expect(await hub.hubHasHeads(docId, heads, 20_000)).toBe(true);
  }
}

test('A: a folder bound to the test hub auto-starts sync, in both directions', async () => {
  // Peer A publishes a project; the workspace is a second copy that VS Code syncs.
  const dirA = await dir('ge8-a-');
  await fs.writeFile(path.join(dirA, 'index.qmd'), '# Home\n\nfrom A\n');
  const a = await HeadlessPeer.publish(dirA, hub.url);
  cleanup.push(() => a.stop());
  await waitForHubHeads(a);

  const workspace = await dir('ge8-ws-');
  // Join with a headless peer to build the folder (project.json naming the
  // hub and the local storage), then stop it so VS Code is the only syncer.
  const seedPeer = await HeadlessPeer.join(hub.url, a.indexDocId, workspace);
  await seedPeer.stop();
  const meta = JSON.parse((await readText(path.join(workspace, '.quarto-hub', 'project.json')))!);
  expect(meta.syncServer).toBe(hub.url);

  vscode = await launchVscode({ workspace });
  const { page } = vscode;

  // An edit published by A appears on disk (the extension activated on
  // workspaceContains:.quarto-hub/project.json and started syncing) ...
  await fs.writeFile(path.join(dirA, 'index.qmd'), '# Home\n\nfrom A, edited\n');
  await waitFor(async () => (await readText(path.join(workspace, 'index.qmd'))) === '# Home\n\nfrom A, edited\n', "A's edit on the workspace disk", 60_000);

  // ... and in the editor.
  await openFile(page, 'index.qmd');
  await expect.poll(() => activeEditorText(page), { timeout: 30_000 }).toContain('from A, edited');

  // A typed edit reaches the hub.
  await page.locator('.editor-instance .monaco-editor').first().click();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+End' : 'Control+End');
  await page.keyboard.type('typed in VS Code');
  const indexDoc = await hub.repo.find<{ files: Record<string, string> }>(`automerge:${a.indexDocId}` as never);
  const fileDocId = indexDoc.doc()!.files['index.qmd']!;
  await waitFor(
    async () => ((await hub.repo.find<{ text: string }>(fileDocId as never)).doc()?.text ?? '').includes('typed in VS Code'),
    'the typed edit on the hub',
    30_000,
  );
  // Observed state, recorded in the plan's notes: hub doc text and the workspace file.
  console.log('hub doc text:', JSON.stringify((await hub.repo.find<{ text: string }>(fileDocId as never)).doc()?.text));
});

test('B: "Quarto Hub: Create Folder from Project" creates the folder, project.json, and the files', async () => {
  const seeder = new Seeder(hub);
  cleanup.push(() => seeder.stop());
  const indexId = await seeder.create({ 'index.qmd': '# Hello\n', 'docs/guide.qmd': 'guide\n' });

  const workspace = await dir('ge8-parent-');
  // The test-only seam: the VS Code layer reads this and passes it as the server.
  vscode = await launchVscode({ workspace, env: { QUARTO_HUB_TEST_SYNC_SERVER: hub.url } });
  const { page } = vscode;
  await runCommand(page, 'Quarto Hub: Create Folder from Project');
  await fillInputBox(page, indexId);

  const created = path.join(workspace, `quarto-hub-${indexId.slice(0, 8)}`);
  await waitFor(async () => (await readText(path.join(created, 'docs', 'guide.qmd'))) === 'guide\n', 'the created folder', 60_000);
  expect(await readText(path.join(created, 'index.qmd'))).toBe('# Hello\n');
  const meta = JSON.parse((await readText(path.join(created, '.quarto-hub', 'project.json')))!);
  expect(meta).toMatchObject({ indexDocId: indexId, syncServer: hub.url });
});
