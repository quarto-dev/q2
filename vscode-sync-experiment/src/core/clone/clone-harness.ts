/**
 * Test-only harness for the clone-mode tests (GE-10): a published git project, a clone of it attached in clone mode,
 * a `ProjectSync` on fakes with a manual clock for the gate, and a second client that registers docs the way another
 * peer on another branch would. Not part of the extension bundle.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { publishGit, type GitPublication } from '../../headless/publish-git';
import { readProjectMeta } from '../meta';
import { ProjectSync, type SyncOptions } from '../project-sync';
import { findDoc, webSocketAdapter } from '../repo';
import { ManualClock, identity, makeFakeHost, sleep, waitFor, type FakeHost } from '../test-support';
import type { TextDoc } from '../text-binding';
import { attachFolder } from './attach-flow';
import { cloneRepo, git, repoWith } from './git-fixtures';
import { registryKey } from '@quarto/quarto-git-epochs';
import { RegistryClient } from './registry-client';

export interface CloneEnv {
  origin: string;
  clone: string;
  pub: GitPublication;
  host: FakeHost;
  clock: ManualClock;
  sync: ProjectSync;
  /** A second client: registers docs under blobs, edits docs. */
  other: OtherPeer;
  /** Advance the manual clock until the controller has decided on the current HEAD (or `condition` holds). */
  settle(condition?: () => boolean | Promise<boolean>): Promise<void>;
  /** Stop the extension and start a new one on the same folder (a window reload). */
  restart(): Promise<void>;
  stop(): Promise<void>;
}

export class OtherPeer {
  readonly repo: Repo;
  private index!: DocHandle<IndexDocument>;
  private registry!: RegistryClient;
  constructor(private readonly hub: TestHub) {
    this.repo = new Repo({ network: [webSocketAdapter(hub.url)] });
  }
  async open(indexDocId: string): Promise<void> {
    this.index = await findDoc<IndexDocument>(this.repo, indexDocId);
    this.registry = new RegistryClient(this.repo, this.index);
    await this.registry.ensure();
  }
  /** Register a new doc with `text` for `(path, blob)`, as a peer on another branch would. Returns the doc id. */
  async register(p: string, blob: string, text: string, gen?: number): Promise<string> {
    const h = this.repo.create<TextDoc>({ text });
    this.registry.register({ path: p, blob, doc: h.documentId, gen: gen ?? this.registry.nextGeneration(p) });
    await this.flush();
    return h.documentId;
  }
  /** Register an existing doc under another `(path, blob)`: the same doc live under two paths. */
  async registerExisting(p: string, blob: string, doc: string, gen: number): Promise<void> {
    this.registry.register({ path: p, blob, doc, gen });
    await this.flush();
  }
  async registerBinary(p: string, blob: string, content: Uint8Array, hash: string): Promise<string> {
    const h = this.repo.create<unknown>({ content, mimeType: 'image/png', hash });
    this.registry.register({ path: p, blob, doc: h.documentId, gen: this.registry.nextGeneration(p) });
    await this.flush();
    return h.documentId;
  }
  retire(p: string, blob: string, doc: string): Promise<void> {
    this.registry.retire({ path: p, blob, doc });
    return this.flush();
  }
  /** Remove a registry entry outright (test only: no real peer does this), to exercise the repair rule. */
  async dropEntry(p: string, blob: string, doc: string): Promise<void> {
    const sidecar = await this.registry.ensure();
    sidecar.change((d) => void delete d.entries[registryKey(p, blob, doc)]);
    await this.flush();
  }
  async edit(docId: string, text: string): Promise<void> {
    const h = await findDoc<TextDoc>(this.repo, docId);
    h.change((d) => void (d.text = text));
    await this.flush();
  }
  async flush(): Promise<void> {
    for (const id of Object.keys(this.repo.handles)) {
      const h = this.repo.handles[id as never] as DocHandle<unknown>;
      if (!(await this.hub.hubHasHeads(h.documentId, h.heads() as string[], 15_000))) throw new Error(`hub never received ${h.documentId}`);
    }
  }
  registryView() {
    return this.registry.view();
  }
  async stop(): Promise<void> {
    this.registry?.dispose();
    await this.repo.shutdown().catch(() => {});
  }
}

export interface EnvOptions {
  /** Prepare the clone (a branch, config) before the extension starts. */
  prepare?: (clone: string, origin: string) => void | Promise<void>;
  /** Do not start the sync; the caller does. */
  noStart?: boolean;
  /** Publish with no registry sidecar (a project whose first git-aware peer creates it). */
  noRegistry?: boolean;
  options?: SyncOptions;
  /** The owner hook (GE-12 supplies the real one). */
  pointer?: import('./pointer-writer').PointerWriter;
}

export async function makeCloneEnv(hub: TestHub, files: Record<string, string | Buffer>, opts: EnvOptions = {}): Promise<CloneEnv> {
  const origin = repoWith(files);
  const pub = await publishGit(origin, hub.url, hub, { registry: !opts.noRegistry });
  const clone = cloneRepo(origin);
  await opts.prepare?.(clone, origin);
  const attached = await attachFolder({ dir: clone, serverUrl: hub.url, indexDocId: pub.indexDocId });
  if (!attached.ok) throw new Error(`attach refused: ${attached.message}`);
  const meta = (await readProjectMeta(clone))!;
  const host = makeFakeHost();
  const clock = new ManualClock();
  const sync = new ProjectSync(clone, meta, identity, host, { ...opts.options, clone: { clock, pointer: opts.pointer } });
  const other = new OtherPeer(hub);
  await other.open(pub.indexDocId);
  if (!opts.noStart) await sync.start();
  const env: CloneEnv = {
    origin,
    clone,
    pub,
    host,
    clock,
    sync,
    other,
    async settle(condition) {
      const head = git(clone, 'rev-parse', 'HEAD');
      await waitFor(
        async () => {
          await env.clock.advance(300);
          await sleep(20);
          const s = env.sync.cloneState();
          return condition ? condition() : s?.head === head && !s.gateClosed;
        },
        'the clone controller to decide',
        20_000,
      );
    },
    async restart() {
      await env.sync.dispose().catch(() => {});
      env.host.notifier.messages.length = 0;
      env.sync = new ProjectSync(clone, (await readProjectMeta(clone))!, identity, host, { ...opts.options, clone: { clock, pointer: opts.pointer } });
      await env.sync.start();
    },
    async stop() {
      await env.sync.dispose().catch(() => {});
      await other.stop();
      await pub.stop();
    },
  };
  return env;
}

export const abs = (env: CloneEnv, rel: string) => path.join(env.clone, rel);
export const readDisk = (env: CloneEnv, rel: string) => fs.readFile(abs(env, rel), 'utf8').catch(() => null);
export { git };

/** The user edits a file on disk; the fake watcher reports it as the real one would. */
export async function editDisk(env: CloneEnv, rel: string, text: string): Promise<void> {
  await fs.writeFile(abs(env, rel), text);
  env.host.watcher.fire('change', abs(env, rel));
}
