/**
 * The owner's pointer writer (GE-12, G16, G13): the originating folder re-points the project's `files` map for the
 * followers when its checkout changes the version of a file. One call is one index change: the batch's flips, relists,
 * additions, resets, and removals land together, so a follower never sees half a switch.
 *
 * Before the change it waits (up to `waitMs`) until the server holds the heads of the docs the change points at, so a
 * follower never meets a pointer to a doc the server lacks. After the wait it writes regardless: a follower treats an
 * unavailable doc as a notice, not a failure (GE-6).
 */
import type { DocHandle, Repo } from '@automerge/automerge-repo/slim';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import { asString } from '@quarto/quarto-git-epochs';
import type { PointerBatch, PointerWriter } from '../clone/pointer-writer';
import { waitForServerHeads, type HeadsWait } from './wait-heads';

/** GE-12 / G32 (detail chosen by the second reviewer): the owner's wait for the server before writing the pointer. */
export const OWNER_WAIT_MS = 10_000;

export interface OwnerWriterDeps {
  repo: Repo;
  index: DocHandle<IndexDocument>;
  log?(line: string): void;
  waitMs?: number;
  /** Replaces the heads wait (tests). */
  waitForHeads?(handles: DocHandle<unknown>[], ms: number): Promise<HeadsWait>;
}

export class OwnerPointerWriter implements PointerWriter {
  private queue: Promise<void> = Promise.resolve();

  constructor(private readonly d: OwnerWriterDeps) {}

  applyDecision(batch: PointerBatch): Promise<void> {
    const run = this.queue.then(() => this.apply(batch));
    this.queue = run.catch(() => {});
    return run;
  }

  private async apply(batch: PointerBatch): Promise<void> {
    const target = new Map<string, string>();
    for (const f of batch.flips) target.set(f.path, f.docId);
    for (const r of batch.relists) target.set(r.path, r.docId);
    for (const r of batch.resets) target.set(r.path, r.newDocId);
    const files = (this.d.index.doc()?.files ?? {}) as Record<string, unknown>;
    const current = (p: string) => (files[p] == null ? undefined : asString(files[p]));
    const sets = [...target].filter(([p, id]) => current(p) !== id);
    const removals = batch.removals.filter((p) => !target.has(p) && current(p) !== undefined);
    if (sets.length === 0 && removals.length === 0) return;

    if (sets.length > 0) {
      const handles: DocHandle<unknown>[] = [];
      for (const [, id] of sets) {
        const h = (this.d.repo.handles as Record<string, DocHandle<unknown> | undefined>)[id];
        if (h) handles.push(h);
      }
      const ms = this.d.waitMs ?? OWNER_WAIT_MS;
      const wait = await (this.d.waitForHeads ? this.d.waitForHeads(handles, ms) : waitForServerHeads(this.d.repo, handles, ms));
      if (!wait.ok) this.d.log?.(`owner: the server has not confirmed ${wait.pending.length} new doc(s) after ${ms} ms; writing the pointer anyway`);
    }
    this.d.index.change((doc) => {
      for (const [p, id] of sets) doc.files[p] = id;
      for (const p of removals) delete doc.files[p];
    });
    this.d.log?.(`owner: pointer change: ${sets.length} set, ${removals.length} removed`);
  }
}
