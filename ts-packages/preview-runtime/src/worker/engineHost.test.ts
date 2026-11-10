import { describe, expect, it, vi } from 'vitest';
import { Rpc } from './rpc';
import { serveEngine, type EngineApi } from './engineHost';

function deferred<T = void>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

/** A fake engine over a plain map, recording the order of operations. */
function fakeEngine() {
  const files = new Map<string, string>();
  const log: string[] = [];
  const gates: Array<ReturnType<typeof deferred<void>>> = [];
  const engine: EngineApi = {
    initWasm: vi.fn(async () => {}),
    vfsAddFile: (path, content) => {
      log.push(`write ${path}=${content}`);
      files.set(path, content);
      return { success: true };
    },
    vfsAddBinaryFile: (path) => {
      log.push(`write-binary ${path}`);
      return { success: true };
    },
    vfsRemoveFile: (path) => ({ success: files.delete(path) }),
    vfsClear: () => {
      files.clear();
      return { success: true };
    },
    vfsListFiles: () => ({ success: true, files: [...files.keys()] }),
    vfsReadFile: (path) =>
      files.has(path) ? { success: true, content: files.get(path) } : { success: false },
    vfsReadBinaryFile: () => ({ success: false }),
    renderPageForPreview: async (path) => {
      const source = files.get(path) ?? '';
      log.push(`render start (${source})`);
      const gate = deferred<void>();
      gates.push(gate);
      await gate.promise;
      files.set('/.quarto/project-artifacts/styles.css', `css for ${source}`);
      log.push('render end');
      return { success: true, ast_json: '{}', renderedSource: source };
    },
    resolvePandocFormats: () => ({ success: true, key: 'html', class: 'preview' }) as never,
    parseQmdContentSync: (text) => ({ success: true, ast: `ast(${text})` }) as never,
    applyNodeEdit: (content) => `${content}!`,
    regenerateNestedBuffers: () => ({ a: 'b' }),
    vfsSetMissHandler: vi.fn(),
  };
  return { engine, files, log, gates };
}

function connect(engine: EngineApi) {
  const channel = new MessageChannel();
  serveEngine(channel.port2, engine);
  const main = new Rpc(channel.port1, { rasterize: async () => new Uint8Array([1]) });
  return { main, close: () => channel.port1.close() };
}

const tick = () => new Promise((r) => setTimeout(r, 5));

describe('serveEngine', () => {
  it('applies a write that arrives during a render after it and before the next render', async () => {
    const { engine, log, gates } = fakeEngine();
    const { main, close } = connect(engine);
    await main.call('vfsAddFile', ['a.qmd', 'v1']);

    const r1 = main.call('renderPreview', ['a.qmd']);
    await tick();
    void main.call('vfsAddFile', ['a.qmd', 'v2']);
    const r2 = main.call('renderPreview', ['a.qmd']);
    await tick();
    expect(log).toEqual(['write a.qmd=v1', 'render start (v1)']);

    gates[0].resolve(undefined);
    await r1;
    await tick();
    gates[1].resolve(undefined);
    await r2;
    expect(log).toEqual([
      'write a.qmd=v1',
      'render start (v1)',
      'render end',
      'write a.qmd=v2',
      'render start (v2)',
      'render end',
    ]);
    close();
  });

  it('carries back only the artifacts a render changed', async () => {
    const { engine, gates } = fakeEngine();
    const { main, close } = connect(engine);
    await main.call('vfsAddFile', ['a.qmd', 'v1']);

    const first = main.call<{ artifacts: { changed: unknown[] } }>('renderPreview', ['a.qmd']);
    await tick();
    gates[0].resolve(undefined);
    expect((await first).artifacts.changed).toEqual([
      { path: '/.quarto/project-artifacts/styles.css', text: 'css for v1' },
    ]);

    const second = main.call<{ artifacts: { changed: unknown[] } }>('renderPreview', ['a.qmd']);
    await tick();
    gates[1].resolve(undefined);
    expect((await second).artifacts.changed).toEqual([]);
    close();
  });

  it('initializes with the host flags and answers the pure helpers', async () => {
    const { engine } = fakeEngine();
    const { main, close } = connect(engine);
    const g = globalThis as Record<string, unknown>;
    delete g.__Q2_EPHEMERAL_STORAGE__;
    await main.call('init', [{ ephemeralStorage: true, perf: false }]);
    expect(g.__Q2_EPHEMERAL_STORAGE__).toBe(true);
    expect(engine.initWasm).toHaveBeenCalled();
    await expect(main.call('parseQmdContent', ['x'])).resolves.toEqual({ success: true, ast: 'ast(x)' });
    await expect(main.call('applyNodeEdit', ['c', '', '', ''])).resolves.toBe('c!');
    await expect(main.call('regenerateNestedBuffers', ['c', ''])).resolves.toEqual({ a: 'b' });
    delete g.__Q2_EPHEMERAL_STORAGE__;
    close();
  });

  it('installs the miss handler and returns the files a render pulled', async () => {
    const { engine, files } = fakeEngine();
    let onPull: ((path: string, bytes: Uint8Array) => void) | undefined;
    const channel = new MessageChannel();
    serveEngine(channel.port2, engine, {
      createMissHandler: (_base, options) => {
        onPull = options?.onPull;
        return {
          read: () => null,
          stat: () => ({ exists: false, isFile: false, isDir: false }),
          list: () => null,
          counts: () => ({ 'read.200': 1 }),
        };
      },
    });
    const main = new Rpc(channel.port1, {});
    await main.call('init', [{ ephemeralStorage: false, perf: false, missBaseUrl: 'http://h/api/preview/' }]);
    expect(engine.vfsSetMissHandler).toHaveBeenCalled();

    // The render pulls a file (the engine calls the handler, which reports it).
    engine.renderPageForPreview = async () => {
      onPull?.('refs.bib', new Uint8Array([64]));
      return { success: true, ast_json: '{}', renderedSource: '' };
    };
    files.set('a.qmd', 'x');
    const result = await main.call<{ pulled: unknown[] }>('renderPreview', ['a.qmd']);
    expect(result.pulled).toEqual([{ path: 'refs.bib', bytes: new Uint8Array([64]) }]);
    await expect(main.call('missStats', [])).resolves.toEqual({ 'read.200': 1 });
    channel.port1.close();
  });
});
