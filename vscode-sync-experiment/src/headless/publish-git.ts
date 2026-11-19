/**
 * Test code only (GE-10; GE-12 builds the real origination): publish a git working tree as a version 4 project. The
 * index carries the sidecar pointer and the `repo` identity, and the registry has one entry per tracked file under its
 * HEAD blob at generation 0, with one doc per file holding the committed content.
 */
import { createHash } from 'node:crypto';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import { createIndexDocumentContent, inferMimeType, isBinaryExtension, type IndexDocument } from '@quarto/quarto-automerge-schema';
import { createGitProbe } from '@quarto/quarto-git-epochs/probe';
import { RegistryClient } from '../core/clone/registry-client';
import { isIgnoredPath } from '../core/ignore';
import { findDoc, webSocketAdapter } from '../core/repo';
import { ensureWasm } from '../core/wasm';
import type { HubLike } from './owner';

export interface GitPublication {
  indexDocId: string;
  sidecarId: string;
  /** Registry entries written, by project-relative path. */
  docs: Record<string, { docId: string; blob: string }>;
  stop(): Promise<void>;
}

/** Publish the tracked files of the repository at `dir` (a directory inside it: the project is that directory). */
export async function publishGit(dir: string, serverUrl: string, hub?: HubLike, opts: { registry?: boolean } = {}): Promise<GitPublication> {
  const withRegistry = opts.registry !== false;
  await ensureWasm();
  const probe = createGitProbe({ cwd: dir });
  await probe.ready();
  const [tracked, roots, prefix] = await Promise.all([probe.lsTree(), probe.rootCommits(), probe.showPrefix()]);
  const repo = new Repo({ network: [webSocketAdapter(serverUrl)] });
  const index = repo.create<IndexDocument>(createIndexDocumentContent());
  const registry = new RegistryClient(repo, index);
  if (withRegistry) await registry.ensure();
  const docs: GitPublication['docs'] = {};
  const handles: DocHandle<unknown>[] = [index as DocHandle<unknown>];
  for (const f of tracked) {
    if (isIgnoredPath(f.path, new Set())) continue;
    const bytes = await fs.readFile(path.join(dir, f.path));
    const content = (await probe.catBlobs([f.blob])).get(f.blob) ?? bytes;
    const handle = isBinaryExtension(f.path)
      ? repo.create<unknown>({ content: new Uint8Array(content), mimeType: inferMimeType(f.path), hash: createHash('sha256').update(content).digest('hex') })
      : repo.create<unknown>({ text: Buffer.from(content).toString('utf8') });
    handles.push(handle as DocHandle<unknown>);
    docs[f.path] = { docId: handle.documentId, blob: f.blob };
    index.change((d) => void (d.files[f.path] = handle.documentId));
    if (withRegistry) registry.register({ path: f.path, blob: f.blob, doc: handle.documentId, gen: 0 });
  }
  index.change((d) => {
    (d as { repo?: unknown }).repo = { roots, prefix };
  });
  const sidecar = withRegistry ? await registry.ensure() : null;
  if (sidecar) handles.push(sidecar as DocHandle<unknown>);
  if (hub) {
    for (const h of handles) {
      if (!(await hub.hubHasHeads(h.documentId, h.heads() as string[], 15_000))) throw new Error(`hub never received ${h.documentId}`);
    }
  } else {
    await new Promise((r) => setTimeout(r, 1500)); // no way to ask a remote hub for its heads here
  }
  return {
    indexDocId: index.documentId,
    sidecarId: sidecar?.documentId ?? '',
    docs,
    async stop() {
      registry.dispose();
      await repo.shutdown().catch(() => {});
    },
  };
}

export { findDoc };
