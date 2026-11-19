/** GE-11 specs 1 to 4 and 7 to 9: input classification, URL derivation, root comparison, offers, health, index inspection, binding. */
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  bindFolder,
  classifyJoinInput,
  deriveWsUrl,
  fetchHealth,
  healthUrlFor,
  inspectIndex,
  isLoopbackUrl,
  offeredModes,
  projectRootMatches,
  type OfferInput,
} from './join';
import { createProjectFolder } from './create-folder';
import { findDoc, makeRepo } from './repo';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import { readProjectMeta } from './meta';
import { MemoryLog, Seeder, startHub, tempDir } from './test-support';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import http from 'node:http';
import { execFileSync } from 'node:child_process';

describe('classifyJoinInput (spec 1)', () => {
  it('classifies a ticket', () => {
    expect(classifyJoinInput('q2previewAbc_123-xyz')).toEqual({ kind: 'ticket', ticket: 'q2previewAbc_123-xyz' });
  });
  it('trims whitespace', () => {
    expect(classifyJoinInput('  q2previewabc \n')).toEqual({ kind: 'ticket', ticket: 'q2previewabc' });
    expect(classifyJoinInput('  ws://127.0.0.1:3000/ws ')).toEqual({ kind: 'ws', url: 'ws://127.0.0.1:3000/ws' });
  });
  it('passes ws and wss URLs through as given', () => {
    expect(classifyJoinInput('wss://hub.example.com/ws')).toEqual({ kind: 'ws', url: 'wss://hub.example.com/ws' });
  });
  it('derives a ws URL from an http or https address', () => {
    expect(classifyJoinInput('http://127.0.0.1:3000/')).toEqual({ kind: 'http', url: 'ws://127.0.0.1:3000/ws' });
    expect(classifyJoinInput('https://hub.example.com/some/page')).toEqual({ kind: 'http', url: 'wss://hub.example.com/ws' });
  });
  it('reads the id and server from a share link', () => {
    expect(classifyJoinInput('https://quarto-hub.com/#/share/3fA4nXRpYK1J?server=wss%3A%2F%2Fquarto-hub.com%2Fws&file=a.qmd&name=x')).toEqual({
      kind: 'share',
      indexDocId: '3fA4nXRpYK1J',
      server: 'wss://quarto-hub.com/ws',
    });
  });
  it('a share link without server= leaves the server unset', () => {
    expect(classifyJoinInput('https://quarto-hub.com/#/share/3fA4nXRpYK1J')).toEqual({ kind: 'share', indexDocId: '3fA4nXRpYK1J' });
  });
  it.each(['', '   ', 'garbage', '3fA4nXRpYK1JPkeyef3KXFMEs4aN', 'automerge:3fA4nXRpYK1J', 'ftp://x/y', 'ws://', 'http://'])('refuses %j', (input) => {
    expect(classifyJoinInput(input)).toEqual({ kind: 'invalid', message: 'That is not a ticket, a URL, or a share link.' });
  });
  it.each(['q2preview x & calc', 'q2previewabc;rm -rf', 'q2preview$(id)', 'q2preview', 'q2previewabc\nnext', 'q2preview"abc'])(
    'refuses a q2preview string with shell metacharacters %j',
    (input) => {
      expect(classifyJoinInput(input).kind).toBe('invalid');
    },
  );
});

describe('URL derivation (spec 2)', () => {
  it('deriveWsUrl maps http to ws and https to wss with the same host and port', () => {
    expect(deriveWsUrl('http://127.0.0.1:3000/')).toBe('ws://127.0.0.1:3000/ws');
    expect(deriveWsUrl('https://hub.example.com')).toBe('wss://hub.example.com/ws');
    expect(deriveWsUrl('http://[::1]:8080/a/b?c=d#e')).toBe('ws://[::1]:8080/ws');
    expect(deriveWsUrl('http://localhost/path')).toBe('ws://localhost/ws');
  });
  it('healthUrlFor swaps the scheme and the path', () => {
    expect(healthUrlFor('ws://127.0.0.1:3000/ws')).toBe('http://127.0.0.1:3000/health');
    expect(healthUrlFor('wss://hub.example.com/ws')).toBe('https://hub.example.com/health');
    expect(healthUrlFor('ws://[::1]:9/ws')).toBe('http://[::1]:9/health');
    expect(healthUrlFor('wss://hub.example.com/some/path')).toBe('https://hub.example.com/health');
  });
  it('isLoopbackUrl accepts 127.0.0.1, localhost, and ::1 only', () => {
    expect(isLoopbackUrl('ws://127.0.0.1:1/ws')).toBe(true);
    expect(isLoopbackUrl('ws://localhost:1/ws')).toBe(true);
    expect(isLoopbackUrl('ws://[::1]:1/ws')).toBe(true);
    expect(isLoopbackUrl('wss://hub.example.com/ws')).toBe(false);
    expect(isLoopbackUrl('ws://127.0.0.2:1/ws')).toBe(false);
    expect(isLoopbackUrl('nonsense')).toBe(false);
  });
});

describe('projectRootMatches (spec 3)', () => {
  let dir: string;
  beforeEach(async () => {
    dir = await tempDir('ge11-root-');
  });
  afterEach(() => fs.rm(dir, { recursive: true, force: true }));

  it('matches the same directory with and without a trailing separator', async () => {
    expect(await projectRootMatches(dir, dir + path.sep)).toBe(true);
    expect(await projectRootMatches(dir + path.sep, dir)).toBe(true);
  });
  it('matches through a symlink, by real path', async () => {
    const link = path.join(os.tmpdir(), `ge11-link-${process.pid}-${Date.now()}`);
    await fs.symlink(dir, link, 'dir');
    try {
      expect(await projectRootMatches(link, dir)).toBe(true);
    } finally {
      await fs.rm(link, { force: true });
    }
  });
  it('does not match a different directory, nor a missing hub root', async () => {
    const other = await tempDir('ge11-other-');
    try {
      expect(await projectRootMatches(dir, other)).toBe(false);
    } finally {
      await fs.rm(other, { recursive: true, force: true });
    }
    expect(await projectRootMatches(dir, undefined)).toBe(false);
  });
  it('folds case on darwin and win32 but not linux (platform injected; paths need not exist)', async () => {
    expect(await projectRootMatches('/Work/Proj', '/work/proj', 'darwin')).toBe(true);
    expect(await projectRootMatches('/Work/Proj', '/work/proj', 'win32')).toBe(true);
    expect(await projectRootMatches('/Work/Proj', '/work/proj', 'linux')).toBe(false);
  });
  it('compares C:\\ with c:/ on win32', async () => {
    expect(await projectRootMatches('C:\\Users\\Me\\proj', 'c:/Users/me/proj/', 'win32')).toBe(true);
    expect(await projectRootMatches('C:\\Users\\Me\\proj', 'D:/Users/me/proj', 'win32')).toBe(false);
  });
});

describe('offeredModes (spec 4)', () => {
  const base: OfferInput = { loopback: false, hasFolder: true, rootMatches: false, gitEnabled: false, passes: false, bound: false };
  const items = (o: Partial<OfferInput>) => offeredModes({ ...base, ...o }).map((m) => m.mode);

  it('loopback, match: editor only (confirm) then mirror, whatever git says', () => {
    for (const git of [false, true]) {
      const r = offeredModes({ ...base, loopback: true, rootMatches: true, gitEnabled: git, passes: git });
      expect(r.map((m) => m.mode)).toEqual(['editor-only', 'mirror']);
      expect(r[0]!.confirm).toBe(true);
    }
  });
  it('loopback, no match, git and passes: attach then mirror', () => {
    expect(items({ loopback: true, gitEnabled: true, passes: true })).toEqual(['clone', 'mirror']);
  });
  it('loopback, no match, not passing: mirror only', () => {
    expect(items({ loopback: true, gitEnabled: true, passes: false })).toEqual(['mirror']);
    expect(items({ loopback: true, gitEnabled: false, passes: false })).toEqual(['mirror']);
  });
  it('remote-looking, match, git and passes: attach, mirror, editor only (never default)', () => {
    const r = offeredModes({ ...base, rootMatches: true, gitEnabled: true, passes: true });
    expect(r.map((m) => m.mode)).toEqual(['clone', 'mirror', 'editor-only']);
    expect(r[2]!.detail).toContain('only if the hub runs on this machine');
    expect(r[2]!.confirm).toBe(true);
  });
  it('remote-looking, match, not passing: mirror then editor only', () => {
    expect(items({ rootMatches: true, gitEnabled: true, passes: false })).toEqual(['mirror', 'editor-only']);
    expect(items({ rootMatches: true, gitEnabled: false })).toEqual(['mirror', 'editor-only']);
  });
  it('remote-looking, no match: attach only when git and passes', () => {
    expect(items({ gitEnabled: true, passes: true })).toEqual(['clone', 'mirror']);
    expect(items({ gitEnabled: true, passes: false })).toEqual(['mirror']);
    expect(items({ gitEnabled: false })).toEqual(['mirror']);
  });
  it('no workspace folder: mirror only, whatever else is true', () => {
    expect(items({ hasFolder: false, rootMatches: true, gitEnabled: true, passes: true })).toEqual(['mirror']);
    expect(items({ hasFolder: false, loopback: true, rootMatches: true })).toEqual(['mirror']);
  });
  it('a bound folder is offered rebind first, then mirror', () => {
    const r = offeredModes({ ...base, bound: true, rootMatches: true, gitEnabled: true, passes: true });
    expect(r.map((m) => m.mode)).toEqual(['rebind', 'mirror']);
    expect(r[0]!.label).toBe('Rebind this folder to the new session');
  });
  it('an unbound no-folder join still has no rebind', () => {
    expect(items({ hasFolder: false, bound: true })).toEqual(['mirror']);
  });
});

describe('fetchHealth (spec 7)', () => {
  let server: http.Server;
  let respond: (req: http.IncomingMessage, res: http.ServerResponse) => void;
  let url: string;
  beforeEach(async () => {
    server = http.createServer((req, res) => respond(req, res));
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    url = `http://127.0.0.1:${(server.address() as { port: number }).port}/health`;
  });
  afterEach(async () => {
    server.closeAllConnections();
    await new Promise((r) => server.close(r));
  });

  it('reads the index doc id and project_root', async () => {
    respond = (_q, res) => res.end(JSON.stringify({ status: 'ok', index_document_id: 'abc', project_root: '/p' }));
    expect(await fetchHealth(url)).toEqual({ indexDocId: 'abc', projectRoot: '/p' });
  });
  it('works without project_root', async () => {
    respond = (_q, res) => res.end(JSON.stringify({ index_document_id: 'abc' }));
    expect(await fetchHealth(url)).toEqual({ indexDocId: 'abc' });
  });
  it('treats a 401, a non-JSON body, a missing id, and a timeout as unreachable', async () => {
    respond = (_q, res) => ((res.statusCode = 401), res.end('{}'));
    expect(await fetchHealth(url)).toBeNull();
    respond = (_q, res) => res.end('<html>nope</html>');
    expect(await fetchHealth(url)).toBeNull();
    respond = (_q, res) => res.end(JSON.stringify({ status: 'ok' }));
    expect(await fetchHealth(url)).toBeNull();
    respond = () => {};
    expect(await fetchHealth(url, 200)).toBeNull();
  });
  it('treats a refused connection as unreachable', async () => {
    expect(await fetchHealth('http://127.0.0.1:1/health', 500)).toBeNull();
  });
});

describe('inspectIndex (spec 8)', () => {
  let hub: TestHub;
  let seeder: Seeder;
  beforeEach(async () => {
    hub = await startHub();
    seeder = new Seeder(hub);
  });
  afterEach(async () => {
    await seeder.stop();
    await hub.stop();
  });

  it('reports a share with a repo entry as git-enabled', async () => {
    const id = await seeder.create({ 'index.qmd': 'hi' });
    seeder.index.change((d) => void ((d as any).repo = { roots: ['abc'], prefix: '' }));
    await seeder.flush();
    expect(await inspectIndex(hub.url, id)).toEqual({ gitEnabled: true, repo: { roots: ['abc'], prefix: '' } });
  });
  it('reports a share without one as not git-enabled', async () => {
    const id = await seeder.create({ 'index.qmd': 'hi' });
    expect(await inspectIndex(hub.url, id)).toEqual({ gitEnabled: false });
  });
  it('says "Project not found on this server." for an unknown id after the timeout', async () => {
    const started = Date.now();
    await expect(inspectIndex(hub.url, '3fA4nXRpYK1JPkeyef3KXFMEs4aN', 700)).rejects.toThrow('Project not found on this server.');
    expect(Date.now() - started).toBeLessThan(5000);
  });
});

describe('the binding writer (spec 9)', () => {
  let hub: TestHub;
  let seeder: Seeder;
  let root: string;
  let indexDocId: string;
  beforeEach(async () => {
    hub = await startHub();
    seeder = new Seeder(hub);
    indexDocId = await seeder.create({ 'index.qmd': 'hi\n', 'a.qmd': 'a\n' });
    root = await tempDir('ge11-bind-');
  });
  afterEach(async () => {
    await seeder.stop();
    await hub.stop();
    await fs.rm(root, { recursive: true, force: true });
  });

  it('writes project.json for editor-only with the server, the mode, and no ticket field', async () => {
    await bindFolder({ root, indexDocId, syncServer: hub.url, mode: 'editor-only' });
    expect(await readProjectMeta(root)).toMatchObject({ indexDocId, syncServer: hub.url, mode: 'editor-only' });
    const raw = await fs.readFile(path.join(root, '.quarto-hub', 'project.json'), 'utf8');
    expect(raw).not.toMatch(/ticket|q2preview/i);
  });
  it('seeds the local store so the first (offline) start finds the index and every file doc, and writes no file', async () => {
    await bindFolder({ root, indexDocId, syncServer: hub.url, mode: 'editor-only' });
    expect((await fs.readdir(root)).sort()).toEqual(['.quarto-hub']);
    const offline = makeRepo(root, null);
    try {
      const index = await findDoc<IndexDocument>(offline, indexDocId, AbortSignal.timeout(5000));
      expect(Object.keys(index.doc()?.files ?? {}).sort()).toEqual(['a.qmd', 'index.qmd']);
      for (const id of Object.values(index.doc()!.files)) expect((await findDoc<{ text: string }>(offline, id, AbortSignal.timeout(5000))).doc()?.text).toBeTruthy();
    } finally {
      await offline.shutdown();
    }
  });
  it('writes nothing and throws when the project cannot be read', async () => {
    await expect(bindFolder({ root, indexDocId: '3fA4nXRpYK1JPkeyef3KXFMEs4aN', syncServer: hub.url, mode: 'editor-only', timeoutMs: 700 })).rejects.toThrow('Project not found');
    expect(await readProjectMeta(root)).toBeNull();
  });
  it('editor-only keeps .quarto-hub out of git status when the folder is a repository', async () => {
    execFileSync('git', ['init', '-q'], { cwd: root });
    await bindFolder({ root, indexDocId, syncServer: hub.url, mode: 'editor-only' });
    expect(await fs.readFile(path.join(root, '.git', 'info', 'exclude'), 'utf8').catch(() => '')).toContain('.quarto-hub');
  });
});

describe('createProjectFolder uses the given server (spec 9)', () => {
  let hub: TestHub;
  let seeder: Seeder;
  let parent: string;
  beforeEach(async () => {
    hub = await startHub();
    seeder = new Seeder(hub);
    parent = await tempDir('ge11-mirror-');
  });
  afterEach(async () => {
    await seeder.stop();
    await hub.stop();
    await fs.rm(parent, { recursive: true, force: true });
  });
  it('records the server in project.json', async () => {
    const id = await seeder.create({ 'index.qmd': 'hi\n' });
    const root = await createProjectFolder(id, parent, new MemoryLog(), { syncServer: hub.url });
    expect((await readProjectMeta(root))?.syncServer).toBe(hub.url);
    expect(await fs.readFile(path.join(root, 'index.qmd'), 'utf8')).toBe('hi\n');
  });
});
