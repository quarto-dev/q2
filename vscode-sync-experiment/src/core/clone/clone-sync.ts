/**
 * Clone mode (GE-10): a folder that holds its own git clone follows its own checkout through the registry instead of
 * merging git's rewrites into a shared doc as if the user had typed them. Everything here is vscode-free; `ProjectSync`
 * supplies a `CloneHost` with the doc and disk operations.
 *
 * Decisions implemented: G9 (arrival), G10 (startup), G13 (removals), G14 (tracked and untracked files), G19 and G21
 * (identity re-verified at startup), G27 (detached files are inert).
 */
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import type { DocHandle, Repo } from '@automerge/automerge-repo/slim';
import { isBinaryDocument, isBinaryExtension, isTextDocument } from '@quarto/quarto-automerge-schema';
import {
  ADOPT_CONFIRM,
  FILES_DETACHED,
  FILES_DETACHED_ACTIONS,
  FILES_JOINED_LIVE,
  FILES_JOINED_LIVE_ACTIONS,
  NUMBERS,
  arrive,
  canonical,
  caseCollisions,
  classify,
  formatNotice,
  liveElsewhere,
  normalizeText,
  startupDecision,
  type ArrivalOutput,
  type ControllerProbe,
  type DecisionAction,
  type EventKind,
  type RegistryView,
} from '@quarto/quarto-git-epochs';
import type { NodeGitProbe } from '@quarto/quarto-git-epochs/probe';
import type { FileDoc } from '../create-folder';
import type { GateClock } from '../editor-gate';
import type { Notifier } from '../host';
import { META_DIR } from '../ignore';
import { type RepoIdentity, verifyIdentity } from './attach';
import { CloneGate } from './gate';
import { type PointerBatch, type PointerWriter, emptyBatch, isEmptyBatch } from './pointer-writer';
import { readRecord, writeRecord } from './record';
import { RegistryClient } from './registry-client';

/** What a clone peer reads from the disk: the comparison key is CRLF-normalized text, or a byte hash. */
export type DiskContent = { kind: 'text'; text: string; key: string } | { kind: 'binary'; bytes: Uint8Array; key: string };

export type CloneEvent = Extract<EventKind, 'switched' | 'detached' | 'reattached' | 'held' | 'reset' | 'retired' | 'moved-forward'>;

/** The doc and disk operations clone mode needs from the sync engine. */
export interface CloneHost {
  root: string;
  repo: Repo;
  notifier: Notifier;
  log(line: string): void;
  handleOf(rel: string): DocHandle<FileDoc> | undefined;
  attachedPaths(): string[];
  /** Make `rel` follow `handle`: moved message on the old doc, untrack, track and rebind; optionally write the disk. */
  switchTo(rel: string, handle: DocHandle<FileDoc>, o: { writeDisk: boolean }): Promise<void>;
  setInert(rel: string, inert: boolean): void;
  createDoc(rel: string, disk: DiskContent): DocHandle<FileDoc>;
  readDisk(rel: string): Promise<DiskContent | null>;
  writeBytes(rel: string, bytes: Uint8Array): Promise<void>;
  /** Startup reconcile for one file: merge offline disk edits into the attached doc, as in a mirror. */
  mergeDiskIntoDoc(rel: string): Promise<void>;
  /** Follow the owner's pointer for the untracked files (G14) after the tracked ones are attached. */
  followPointer(untracked: (rel: string) => boolean): Promise<void>;
  loadDoc(docId: string, signal: AbortSignal): Promise<DocHandle<FileDoc>>;
  logEvent(event: CloneEvent, rel: string, extra: { from?: string; to?: string; reason?: string }): Promise<void>;
  /** The detached or superseded set changed: refresh the status bar. */
  stateChanged?(): void;
}

export interface CloneSyncOptions {
  identity?: RepoIdentity;
  probe: NodeGitProbe;
  controllerProbe?: ControllerProbe;
  clock: GateClock;
  registry: RegistryClient;
  pointer: PointerWriter;
  isIgnored(rel: string): boolean;
}

export class CloneRefusedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CloneRefusedError';
  }
}

const sha = (b: Uint8Array) => createHash('sha256').update(b).digest('hex');

export function docKey(doc: FileDoc | undefined): string | undefined {
  if (!doc) return undefined;
  if (isTextDocument(doc)) return normalizeText(doc.text);
  if (isBinaryDocument(doc)) return doc.hash;
  return undefined;
}

export interface CloneState {
  head: string | null;
  gateClosed: boolean;
  /** rel path -> attached doc id. */
  attached: Record<string, string>;
  detached: string[];
  superseded: string[];
}

export class CloneSync {
  private readonly probe: NodeGitProbe;
  private gate: CloneGate | null = null;
  private head: string | null = null;
  /** Participating (tracked, non-ignored) files and their blob ids at the last decision. */
  private blobs: Record<string, string> = {};
  private readonly detached = new Set<string>();
  private readonly superseded = new Set<string>();
  /** Paths git removed: the doc is never written back over them (G13). */
  private readonly removed = new Set<string>();
  private readonly genOf = new Map<string, number>();
  /** Per-file doc content at the moment the gate went pending: the "last synced" side of the classifier. */
  private snapshot = new Map<string, string>();
  private ownWrites = new Set<string>();
  private lastWritten = new Map<string, string>();
  private reevalWanted = false;
  private deciding = false;
  private disposed = false;
  private batchNotices = { detached: new Set<string>(), joined: new Set<string>() };
  private readonly unsubscribes: (() => void)[] = [];

  constructor(
    private readonly h: CloneHost,
    private readonly o: CloneSyncOptions,
  ) {
    this.probe = o.probe;
  }

  // --- queries ----------------------------------------------------------------------------------------------

  isClosed(): boolean {
    return this.gate?.isClosed() ?? false;
  }
  isInert(rel: string): boolean {
    return this.detached.has(rel);
  }
  /** True while the doc for `rel` may be written to disk (not detached, not held, not absent from HEAD). */
  mayWriteDisk(rel: string): boolean {
    return !this.detached.has(rel) && !this.removed.has(rel) && !this.isClosed();
  }
  isTracked(rel: string): boolean {
    return rel in this.blobs;
  }
  isRemoved(rel: string): boolean {
    return this.removed.has(rel);
  }
  state(): CloneState {
    const attached: Record<string, string> = {};
    for (const rel of this.h.attachedPaths()) attached[rel] = this.h.handleOf(rel)!.documentId;
    return {
      head: this.head,
      gateClosed: this.isClosed(),
      attached,
      detached: [...this.detached].sort(),
      superseded: [...this.superseded].sort(),
    };
  }
  registryView(): RegistryView {
    return this.o.registry.view();
  }
  /** The blob id `rel` had at the last decision. */
  blobOf(rel: string): string | undefined {
    return this.blobs[rel];
  }

  /** Remember the key the extension just wrote to `rel`, so the file's own event does not restart the quiet window. */
  noteWritten(rel: string, key: string): void {
    this.lastWritten.set(rel, key);
  }

  /** A file system event for `rel` (a project file, never `.git` or `.quarto-hub`). */
  async noteFsEvent(rel: string): Promise<void> {
    if (!this.gate) return;
    const written = this.lastWritten.get(rel);
    if (written !== undefined) {
      const disk = await this.h.readDisk(rel).catch(() => null);
      if (disk?.key === written) {
        this.ownWrites.add(rel);
        this.gate.noteActivity(rel);
        this.ownWrites.delete(rel);
        return;
      }
    }
    this.gate.noteActivity(rel);
  }

  // --- startup (G10, G19) ------------------------------------------------------------------------------------

  async start(): Promise<void> {
    const { probe } = this;
    try {
      await probe.ready();
    } catch {
      throw new CloneRefusedError('This folder is not inside a git repository; clone mode needs one.');
    }
    this.head = await probe.headSha();
    if (this.head === null) throw new CloneRefusedError('The repository has no commits yet.');
    await this.o.registry.ensure();
    if (this.o.identity) {
      const v = await verifyIdentity(
        probe,
        this.o.identity,
        this.o.registry.view().entries.map((e) => ({ path: e.path, blob: e.blob })),
      );
      if (!v.ok) throw new CloneRefusedError(`${v.message} Rebind the folder to start over.`);
    }
    this.o.registry.onSidecarSwitched = () => void this.onSidecarSwitched();
    this.unsubscribes.push(this.o.registry.onChange(() => void this.reevaluate()), this.o.registry.follow());

    const tree = await this.participating();
    const record = await readRecord(this.h.root);
    const plan = startupDecision({ record, head: this.head, blobs: tree });
    this.blobs = tree;
    const batch = emptyBatch();
    const blobContent = await this.blobKeys(Object.keys(tree).filter((p) => plan.files[p] === 'arrive').map((p) => tree[p]!));
    for (const [rel, blob] of Object.entries(tree)) {
      try {
        const step = plan.files[rel];
        if (step === 'arrive') await this.arriveFile(rel, blob, batch, blobContent);
        else await this.startupAttach(rel, blob, step === 'stay-detached', batch, blobContent);
      } catch (err) {
        this.h.log(`clone startup failed for ${rel}, falling back to ordinary sync: ${err instanceof Error ? err.message : String(err)}`);
      }
    }
    await this.h.followPointer((rel) => !(rel in this.blobs));
    await this.persist();
    await this.o.pointer.applyDecision(batch).catch((e) => this.h.log(`pointer writer failed: ${String(e)}`));
    await this.flushNotices();

    this.gate = new CloneGate({
      probe: this.o.controllerProbe ?? probe,
      clock: this.o.clock,
      log: (l) => this.h.log(l),
      diskDiffersFromSynced: () => this.snapshotAtTrigger(),
      isOwnWrite: (p) => this.ownWrites.has(p),
      onDecision: (a) => this.decide(a),
    });
    await this.gate.start();
  }

  /** Reconcile (or stay detached) a file whose blob is unchanged since the record. */
  private async startupAttach(rel: string, blob: string, stayDetached: boolean, batch: PointerBatch, blobs: Map<string, string>): Promise<void> {
    const canon = this.o.registry.canonical(rel, blob);
    if (!canon) return this.arriveFile(rel, blob, batch, blobs);
    const handle = await this.loadCanonical(canon.doc);
    if (!handle) return this.arriveFile(rel, blob, batch, blobs);
    this.genOf.set(canon.doc, canon.gen);
    await this.h.switchTo(rel, handle, { writeDisk: false });
    if (stayDetached) {
      this.detach(rel);
      return;
    }
    await this.h.mergeDiskIntoDoc(rel);
  }

  /** Tracked regular files under the project that the sync covers. */
  private async participating(): Promise<Record<string, string>> {
    const out: Record<string, string> = {};
    for (const f of await this.probe.lsTree()) if (!this.o.isIgnored(f.path)) out[f.path] = f.blob;
    return out;
  }

  private async loadCanonical(docId: string): Promise<DocHandle<FileDoc> | null> {
    try {
      return await this.h.loadDoc(docId, AbortSignal.timeout(NUMBERS.unavailableCanonicalMs));
    } catch {
      return null; // unavailable for the whole window: treated as absent for this arrival (G11)
    }
  }

  private async blobKeys(ids: string[]): Promise<Map<string, string>> {
    const out = new Map<string, string>();
    if (ids.length === 0) return out;
    const bytes = await this.probe.catBlobs([...new Set(ids)]);
    // The key depends on the path (text or binary), so keep the raw bytes and key lazily.
    for (const [id, b] of bytes) out.set(id, Buffer.from(b).toString('base64'));
    return out;
  }

  private blobKey(rel: string, blob: string, raw: Map<string, string>): string | undefined {
    const b64 = raw.get(blob);
    if (b64 === undefined) return undefined;
    const bytes = Buffer.from(b64, 'base64');
    return isBinaryExtension(rel) ? sha(bytes) : normalizeText(bytes.toString('utf8'));
  }

  // --- the controller's decision (G2, G9) -----------------------------------------------------------------------

  private snapshotAtTrigger(): string[] {
    this.snapshot = new Map();
    const held: string[] = [];
    for (const rel of this.h.attachedPaths()) {
      if (this.detached.has(rel)) continue;
      const key = docKey(this.h.handleOf(rel)?.doc());
      if (key === undefined) continue;
      this.snapshot.set(rel, key);
      let disk: string | null = null;
      try {
        const buf = readFileSync(path.join(this.h.root, rel));
        disk = isBinaryExtension(rel) ? sha(buf) : normalizeText(buf.toString('utf8'));
      } catch {
        disk = null;
      }
      if (disk !== key) held.push(rel);
    }
    return held;
  }

  private async decide(action: DecisionAction): Promise<void> {
    if (this.disposed) return;
    this.deciding = true;
    const batch = emptyBatch();
    try {
      const tree = await this.participating();
      const head = action.head;
      const jj = this.probe.isColocatedJj();
      const changed = new Set<string>(action.held);
      for (const rel of action.held) await this.h.logEvent('held', rel, { reason: 'disk differed from the synced content when git started' });
      for (const [rel, blob] of Object.entries(tree)) if (this.blobs[rel] !== blob) changed.add(rel);
      for (const rel of this.h.attachedPaths()) {
        if (this.detached.has(rel) || !(rel in tree)) continue;
        const disk = await this.h.readDisk(rel);
        if (disk && disk.key !== (this.snapshot.get(rel) ?? docKey(this.h.handleOf(rel)?.doc()))) changed.add(rel);
      }
      const removedNow = Object.keys(this.blobs).filter((p) => !(p in tree));
      const raw = await this.blobKeys([...changed].filter((p) => p in tree).map((p) => tree[p]!));

      const diskOf = new Map<string, DiskContent | null>();
      for (const rel of changed) diskOf.set(rel, await this.h.readDisk(rel));
      const verdict = classify({
        peer: 'extension',
        reflog: jj ? null : action.reflog,
        changed: [...changed]
          .filter((p) => p in tree)
          .map((rel) => ({
            path: rel,
            disk: diskOf.get(rel)?.key ?? '',
            synced: this.snapshot.get(rel) ?? docKey(this.h.handleOf(rel)?.doc()) ?? '',
            blob: this.blobKey(rel, tree[rel]!, raw) ?? '',
          })),
      });

      const oldBlobs = this.blobs;
      this.head = head;
      this.blobs = tree;
      for (const rel of removedNow) {
        this.removed.add(rel);
        batch.removals.push(rel);
      }
      for (const rel of changed) {
        if (!(rel in tree)) continue;
        const wasRemoved = this.removed.delete(rel);
        try {
          const handle = this.h.handleOf(rel);
          const oldBlob = oldBlobs[rel];
          if (verdict === 'commit' && handle && !this.detached.has(rel) && oldBlob !== undefined) {
            this.o.registry.moveForward({ path: rel, oldBlob, newBlob: tree[rel]!, doc: handle.documentId });
            await this.h.logEvent('moved-forward', rel, { to: handle.documentId });
          } else {
            await this.arriveFile(rel, tree[rel]!, batch, raw, diskOf.get(rel) ?? undefined);
            if (wasRemoved) {
              const d = this.h.handleOf(rel)?.documentId;
              if (d) batch.relists.push({ path: rel, docId: d });
            }
          }
        } catch (err) {
          this.h.log(`clone decision failed for ${rel}, falling back to ordinary sync: ${err instanceof Error ? err.message : String(err)}`);
        }
      }
      this.snapshot.clear();
      await this.persist();
      if (!isEmptyBatch(batch)) await this.o.pointer.applyDecision(batch).catch((e) => this.h.log(`pointer writer failed: ${String(e)}`));
      await this.flushNotices();
    } finally {
      this.deciding = false;
    }
    if (this.reevalWanted) void this.reevaluate();
  }

  // --- arrival for one file (G9) ----------------------------------------------------------------------------------

  private async arriveFile(rel: string, blob: string, batch: PointerBatch, raw: Map<string, string>, diskIn?: DiskContent): Promise<void> {
    const kind = isBinaryExtension(rel) ? 'binary' : 'text';
    const disk = diskIn ?? (await this.h.readDisk(rel));
    if (!disk) return; // the file is absent: nothing to attach to (G14: never create a file the clone lacks)
    let blobKey = this.blobKey(rel, blob, raw);
    if (blobKey === undefined) {
      const fetched = await this.blobKeys([blob]);
      blobKey = this.blobKey(rel, blob, fetched) ?? '';
    }
    const view = this.o.registry.view();
    const canon = canonical(view, rel, blob);
    const attachedHandle = this.h.handleOf(rel);
    let canonHandle: DocHandle<FileDoc> | null = null;
    let unavailable = false;
    if (canon) {
      canonHandle = await this.loadCanonical(canon.doc);
      unavailable = canonHandle === null;
    }
    const T = docKey(canonHandle?.doc());
    const attached: 'none' | 'canonical' | 'other' = !attachedHandle ? 'none' : canon && attachedHandle.documentId === canon.doc ? 'canonical' : 'other';
    const retiredByOther = !!attachedHandle && view.retired.some((r) => r.path === rel && r.doc === attachedHandle.documentId);
    const out = arrive({
      peer: 'extension',
      kind,
      disk: disk.key,
      doc: T,
      blob: blobKey,
      attached,
      retiredByOther,
      jj: this.probe.isColocatedJj(),
      canonicalUnavailable: unavailable,
      liveUnderOtherPath: !!canon && liveElsewhere(view, canon.doc, rel),
      caseCollision: caseCollisions(view).has(rel),
    });
    await this.enact(out, { rel, blob, disk, canon, canonHandle, attachedHandle, batch, T });
  }

  private async enact(
    out: ArrivalOutput,
    c: {
      rel: string;
      blob: string;
      disk: DiskContent;
      canon: { doc: string; gen: number } | null;
      canonHandle: DocHandle<FileDoc> | null;
      attachedHandle: DocHandle<FileDoc> | undefined;
      batch: PointerBatch;
      T: string | undefined;
    },
  ): Promise<void> {
    const { rel, blob, disk, batch } = c;
    const reg = this.o.registry;
    const reason = `step ${out.step}: ${out.action}`;
    switch (out.action) {
      case 'ordinary-sync':
        return;
      case 'stay-live':
        this.superseded.add(rel);
        return;
      case 're-baseline':
        this.attachedClean(rel);
        return;
      case 'retire-create-flip': {
        const old = c.attachedHandle!;
        reg.retire({ path: rel, blob, doc: old.documentId });
        const fresh = this.h.createDoc(rel, disk);
        const gen = reg.nextGeneration(rel);
        reg.register({ path: rel, blob, doc: fresh.documentId, gen });
        this.genOf.set(fresh.documentId, gen);
        await this.h.switchTo(rel, fresh, { writeDisk: false });
        this.attachedClean(rel);
        batch.resets.push({ path: rel, retiredDocId: old.documentId, newDocId: fresh.documentId, discarded: out.discarded ?? 0 });
        await this.h.logEvent('retired', rel, { from: old.documentId, reason });
        await this.h.logEvent('reset', rel, { from: old.documentId, to: fresh.documentId, reason });
        return;
      }
      case 'create-from-disk':
      case 'wait-5s-then-create': {
        const fresh = this.h.createDoc(rel, disk);
        const gen = reg.nextGeneration(rel);
        reg.register({ path: rel, blob, doc: fresh.documentId, gen });
        this.genOf.set(fresh.documentId, gen);
        const from = c.attachedHandle?.documentId;
        await this.h.switchTo(rel, fresh, { writeDisk: false });
        this.attachedClean(rel);
        batch.flips.push({ path: rel, docId: fresh.documentId });
        if (!c.canon && !from) batch.additions.push(rel);
        await this.h.logEvent('switched', rel, { from, to: fresh.documentId, reason });
        return;
      }
      case 'attach-silently':
      case 'join-with-notice': {
        const doc = c.canonHandle!;
        const from = c.attachedHandle?.documentId;
        this.genOf.set(doc.documentId, c.canon!.gen);
        await this.h.switchTo(rel, doc, { writeDisk: out.action === 'join-with-notice' });
        this.attachedClean(rel);
        if (out.action === 'join-with-notice') this.batchNotices.joined.add(rel);
        batch.flips.push({ path: rel, docId: doc.documentId });
        await this.h.logEvent('switched', rel, { from, to: doc.documentId, reason });
        return;
      }
      case 'fork-then-attach':
      case 'create-from-content-no-history': {
        const source = c.canonHandle!;
        const forked =
          out.action === 'fork-then-attach'
            ? (this.h.repo.clone(source) as DocHandle<FileDoc>)
            : this.h.createDoc(rel, { kind: 'text', text: (source.doc() as { text: string }).text, key: c.T ?? '' });
        const gen = reg.nextGeneration(rel);
        reg.register({ path: rel, blob, doc: forked.documentId, gen });
        this.genOf.set(forked.documentId, gen);
        await this.h.switchTo(rel, forked, { writeDisk: false });
        this.attachedClean(rel);
        batch.flips.push({ path: rel, docId: forked.documentId });
        await this.h.logEvent('switched', rel, { from: c.attachedHandle?.documentId, to: forked.documentId, reason });
        return;
      }
      case 'detach-and-ask': {
        if (!c.attachedHandle && c.canonHandle) await this.h.switchTo(rel, c.canonHandle, { writeDisk: false });
        this.detach(rel);
        this.batchNotices.detached.add(rel);
        await this.h.logEvent('detached', rel, { to: c.canonHandle?.documentId, reason });
        return;
      }
      default:
        this.h.log(`clone: unhandled arrival action ${out.action} for ${rel}`);
    }
  }

  private attachedClean(rel: string): void {
    this.detached.delete(rel);
    this.superseded.delete(rel);
    this.h.setInert(rel, false);
  }

  private detach(rel: string): void {
    this.detached.add(rel);
    this.h.setInert(rel, true);
  }

  // --- detached files: adopt, stay, re-attach, use my checkout ---------------------------------------------------------

  /** Replace a detached file with the shared version, asking first when the file differs from its blob. */
  async adopt(rel: string): Promise<boolean> {
    const blob = this.blobs[rel];
    if (blob === undefined) return false;
    const disk = await this.h.readDisk(rel);
    const raw = await this.blobKeys([blob]);
    if (disk && disk.key !== this.blobKey(rel, blob, raw)) {
      const choice = await this.h.notifier.warn(ADOPT_CONFIRM, 'Replace');
      if (choice !== 'Replace') return false;
    }
    const canon = this.o.registry.canonical(rel, blob);
    if (!canon) return false;
    const handle = await this.loadCanonical(canon.doc);
    if (!handle) return false;
    this.genOf.set(canon.doc, canon.gen);
    await this.h.switchTo(rel, handle, { writeDisk: true });
    this.attachedClean(rel);
    await this.persist();
    await this.h.logEvent('reattached', rel, { to: handle.documentId, reason: 'adopted the shared version' });
    return true;
  }

  async stayDetached(rel: string): Promise<void> {
    if (this.detached.has(rel)) await this.persist();
  }

  /** The file's text equals the shared doc's again: attach without asking. */
  async maybeReattach(rel: string): Promise<void> {
    if (!this.detached.has(rel) || this.isClosed() || this.deciding) return;
    const blob = this.blobs[rel];
    const disk = await this.h.readDisk(rel);
    if (blob === undefined || !disk) return;
    const canon = this.o.registry.canonical(rel, blob);
    if (!canon) return;
    const handle = await this.loadCanonical(canon.doc);
    if (!handle || docKey(handle.doc()) !== disk.key) return;
    this.genOf.set(canon.doc, canon.gen);
    await this.h.switchTo(rel, handle, { writeDisk: false });
    this.attachedClean(rel);
    await this.persist();
    await this.h.logEvent('reattached', rel, { to: handle.documentId, reason: 'contents became equal' });
  }

  /** "Use my checkout instead": write the blob's content back (no filters) and detach; never creates a second doc. */
  async useMyCheckout(rel: string): Promise<void> {
    const blob = this.blobs[rel];
    if (blob === undefined) return;
    const bytes = (await this.probe.catBlobs([blob])).get(blob);
    if (!bytes) return;
    this.detach(rel);
    await this.h.writeBytes(rel, bytes);
    await this.persist();
    await this.h.logEvent('detached', rel, { reason: 'use my checkout instead' });
  }

  // --- the registry changed (G7) --------------------------------------------------------------------------------------

  private async onSidecarSwitched(): Promise<void> {
    for (const rel of this.h.attachedPaths()) {
      const h = this.h.handleOf(rel);
      const blob = this.blobs[rel];
      if (h && blob !== undefined && !this.detached.has(rel)) {
        this.o.registry.repair({ path: rel, doc: h.documentId, gen: this.genOf.get(h.documentId) ?? 0 }, blob);
      }
    }
    await this.reevaluate();
  }

  /** After a sidecar change: repair, then re-evaluate every attached file against its canonical doc. */
  async reevaluate(): Promise<void> {
    if (this.disposed) return;
    if (this.deciding || this.isClosed()) {
      this.reevalWanted = true;
      return;
    }
    this.reevalWanted = false;
    const batch = emptyBatch();
    for (const rel of [...this.h.attachedPaths()]) {
      const handle = this.h.handleOf(rel);
      const blob = this.blobs[rel];
      if (!handle || blob === undefined || this.detached.has(rel)) continue;
      const view = this.o.registry.view();
      if (view.retired.some((r) => r.path === rel && r.doc === handle.documentId)) {
        this.superseded.add(rel);
        continue;
      }
      if (!view.entries.some((e) => e.path === rel && e.doc === handle.documentId)) {
        this.o.registry.repair({ path: rel, doc: handle.documentId, gen: this.genOf.get(handle.documentId) ?? 0 }, blob);
        continue;
      }
      const canon = canonical(this.o.registry.view(), rel, blob);
      if (!canon || canon.doc === handle.documentId) continue;
      try {
        await this.arriveFile(rel, blob, batch, new Map());
      } catch (err) {
        this.h.log(`clone re-evaluation failed for ${rel}: ${err instanceof Error ? err.message : String(err)}`);
      }
    }
    if (!isEmptyBatch(batch)) await this.o.pointer.applyDecision(batch).catch(() => {});
    await this.persist();
    await this.flushNotices();
  }

  // --- notices and the record ------------------------------------------------------------------------------------------

  /** One message per kind per batch. The choice is handled when it arrives; the gate never waits for the user. */
  private async flushNotices(): Promise<void> {
    const { detached, joined } = this.batchNotices;
    this.batchNotices = { detached: new Set(), joined: new Set() };
    if (detached.size > 0) {
      void this.h.notifier.warn(formatNotice(FILES_DETACHED, { n: detached.size }), ...FILES_DETACHED_ACTIONS).then(async (choice) => {
        if (choice === FILES_DETACHED_ACTIONS[0]) for (const rel of detached) await this.adopt(rel);
      });
    }
    if (joined.size > 0) {
      void this.h.notifier.warn(formatNotice(FILES_JOINED_LIVE, { n: joined.size }), ...FILES_JOINED_LIVE_ACTIONS).then(async (choice) => {
        if (choice === FILES_JOINED_LIVE_ACTIONS[0]) for (const rel of joined) await this.useMyCheckout(rel);
      });
    }
  }

  async persist(): Promise<void> {
    this.h.stateChanged?.();
    if (this.head === null) return;
    await writeRecord(this.h.root, { head: this.head, blobs: this.blobs, detached: [...this.detached] }).catch((e) =>
      this.h.log(`could not write ${META_DIR}/git-epochs.json: ${String(e)}`),
    );
  }

  async dispose(): Promise<void> {
    this.disposed = true;
    this.gate?.dispose();
    this.unsubscribes.splice(0).forEach((u) => u());
    this.o.registry.dispose();
    await this.persist();
  }
}
