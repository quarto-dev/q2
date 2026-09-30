/**
 * Phase 0 integration spike for the author-ID transition
 * (epic bd-o1yn1fqy, strand bd-c0y8i71s; plan:
 * claude-notes/plans/2026-09-30-automerge-author-id-transition.md).
 *
 * Source reading of `@automerge/automerge` 3.5.0 and
 * `@automerge/automerge-repo` 2.6.0-alpha.5 answered the option-plumbing
 * questions (`clone` propagates author, `Repo.import` drops it). This
 * spike guards what reading could not — and it caught two divergences
 * from the plan's API-surface summary, verified here at runtime:
 *
 * 1. **Author footers are per-actor, not per-change.** In automerge
 *    0.12.0, `transaction_args` attaches the author only when `seq == 1`
 *    (automerge.rs:568), and `set_author` mints a fresh random actor
 *    whenever the author value changes (automerge.rs:389-396). So each
 *    actor carries the author on exactly its first change; later changes
 *    by the same actor have `author` absent and are attributed via the
 *    actor→author index (`getAuthorForActor`), which is rebuilt from
 *    history on load (change_graph.rs:908-911). The reader-side
 *    resolution rule is therefore
 *    `change.author ?? getAuthorForActor(doc, change.actor) ?? change.actor`,
 *    not the plan's original `change.author ?? change.actor`.
 *
 * 2. **`DecodedChange.author` is `undefined` (not `null`) when absent**,
 *    despite the `.d.ts` declaring `Author | null`. `??` handles both.
 *
 * Also pinned: `handle.docSync()` does not exist in automerge-repo
 * 2.6.0-alpha.5 — the synchronous accessor is `handle.doc()`, so the D9
 * escape hatch is `getBackend(handle.doc()!).setAuthor(author)`.
 */

import { describe, it, expect } from 'vitest';
import {
  from,
  load,
  change,
  clone,
  save,
  decodeChange,
  getAllChanges,
  getLastLocalChange,
  getActorId,
  getAuthor,
  getAuthors,
  getAuthorForActor,
  getBackend,
  type Doc,
} from '@automerge/automerge';
import { Repo } from '@automerge/automerge-repo';

// Same shape as sub_to_actor_id_for_project's HMAC-SHA256 output: 64 hex
// chars, so the server-minted value is a valid author with no re-encoding
// (decision D5).
const AUTHOR = 'a'.repeat(64);

type TextDoc = { text: string };

/** Author footer on the last locally-authored change (`undefined` when absent). */
function lastChangeAuthor(doc: Doc<TextDoc>): string | null | undefined {
  const bytes = getLastLocalChange(doc);
  expect(bytes).toBeDefined();
  return decodeChange(bytes!).author;
}

describe('author application via the getBackend escape hatch (D9)', () => {
  it('setAuthor on a repo handle backend re-actors the doc and stamps the first handle.change', async () => {
    const repo = new Repo();
    // Repo.import recreates the doc via Automerge.load(binary) with no
    // options — any author set before import would be dropped, exactly as
    // the actor is today, so the author must be applied after the handle
    // is ready.
    const handle = repo.import<TextDoc>(save(from<TextDoc>({ text: '' })));
    await handle.whenReady();

    const doc = handle.doc();
    expect(doc).toBeDefined();
    const actorBefore = getActorId(doc!);
    getBackend(doc!).setAuthor(AUTHOR);

    // set_author mints a fresh random actor when the author value changes
    // (None -> AUTHOR here): the author is recorded on the new actor's
    // first change, giving a 1 author -> N actors relationship.
    const actorAfter = getActorId(handle.doc()!);
    expect(actorAfter).not.toBe(actorBefore);

    handle.change((d) => {
      d.text = 'edited through the repo handle';
    });

    const after1 = handle.doc()!;
    expect(getActorId(after1)).toBe(actorAfter);
    expect(lastChangeAuthor(after1)).toBe(AUTHOR);
    expect(getAuthor(after1)).toBe(AUTHOR);
    expect(getAuthorForActor(after1, getActorId(after1))).toBe(AUTHOR);
    expect(getAuthors(after1)).toEqual([AUTHOR]);

    // seq > 1: the change itself carries no footer; attribution flows
    // through the actor->author index. Phase 4's resolution rule must be
    // author ?? getAuthorForActor(actor) ?? actor.
    handle.change((d) => {
      d.text = 'second edit';
    });
    const after2 = handle.doc()!;
    expect(lastChangeAuthor(after2)).toBeUndefined();
    expect(getAuthorForActor(after2, getActorId(after2))).toBe(AUTHOR);

    // Re-applying the same author (findDoc on an already-author-set doc)
    // is a no-op: no actor churn, no duplicate stamping.
    getBackend(after2).setAuthor(AUTHOR);
    expect(getActorId(handle.doc()!)).toBe(actorAfter);
  });

  it('the documented clone fallback (handle.update with clone(doc, { author })) stamps identically', async () => {
    const repo = new Repo();
    const handle = repo.import<TextDoc>(save(from<TextDoc>({ text: '' })));
    await handle.whenReady();

    handle.update((doc) => clone(doc, { author: AUTHOR }));
    handle.change((d) => {
      d.text = 'edited after clone fallback';
    });

    const after = handle.doc()!;
    expect(lastChangeAuthor(after)).toBe(AUTHOR);
    expect(getAuthorForActor(after, getActorId(after))).toBe(AUTHOR);
  });

  it('a doc round-tripped through Repo.import with no author applied stays authorless (legacy simulation)', async () => {
    const repo = new Repo();
    const handle = repo.import<TextDoc>(save(from<TextDoc>({ text: '' })));
    await handle.whenReady();

    handle.change((d) => {
      d.text = 'legacy edit';
    });

    const doc = handle.doc()!;
    expect(lastChangeAuthor(doc)).toBeUndefined();
    expect(getAuthor(doc)).toBeFalsy();
    // Runtime delivers undefined (not the .d.ts's null) for absence.
    expect(getAuthorForActor(doc, getActorId(doc))).toBeUndefined();
  });
});

describe('author footers are per-actor and survive persistence', () => {
  it('from(init, { author }) stamps only the first change; the actor->author mapping survives save/load', () => {
    // This is the createDoc shape Phase 2 adopts: automergeFrom(init,
    // { author }) attributes the very first change.
    let doc = from<TextDoc>({ text: '' }, { author: AUTHOR });
    doc = change(doc, (d) => {
      d.text = 'one';
    });
    doc = change(doc, (d) => {
      d.text = 'two';
    });

    const authored = getAllChanges(doc).map((c) => decodeChange(c).author);
    // Exactly one change carries the footer: the actor's seq-1 change
    // (the initial `from` change).
    expect(authored.filter((a) => a === AUTHOR)).toHaveLength(1);

    const actor = getActorId(doc);
    const reloaded = load<TextDoc>(save(doc));

    // The actor->author mapping is rebuilt from history on load...
    expect(getAuthorForActor(reloaded, actor)).toBe(AUTHOR);
    expect(getAuthors(reloaded)).toEqual([AUTHOR]);
    // ...but the "current author" runtime setting is not: a reloaded doc
    // must have the author re-applied (via setAuthor) before its next
    // change, or the change is authorless.
    expect(getAuthor(reloaded)).toBeFalsy();
    const afterReload = change(reloaded, (d) => {
      d.text = 'three';
    });
    expect(lastChangeAuthor(afterReload)).toBeUndefined();
  });
});

describe('A.load drops the author option (pinned upstream behavior)', () => {
  const bytes = save(from<TextDoc>({ text: '' }));

  it('load(bytes, { author }) then change yields an authorless change', () => {
    let doc = load<TextDoc>(bytes, { author: AUTHOR });
    doc = change(doc, (d) => {
      d.text = 'after plain load';
    });
    expect(lastChangeAuthor(doc)).toBeUndefined();
  });

  it('clone(load(bytes), { author }) then change stamps the author', () => {
    let doc = clone(load<TextDoc>(bytes), { author: AUTHOR });
    doc = change(doc, (d) => {
      d.text = 'after load + clone';
    });
    expect(lastChangeAuthor(doc)).toBe(AUTHOR);
    expect(getAuthor(doc)).toBe(AUTHOR);
  });
});

describe('on-disk cost of the author footer', () => {
  it('measures save() length with and without an author (per-actor, not per-change)', () => {
    const N = 50;
    // Distinct payloads so chunk compression cannot collapse the text
    // itself; the delta isolates the author footer's serialized cost.
    let anonymous = from<TextDoc>({ text: '' });
    for (let i = 0; i < N; i++) {
      anonymous = change(anonymous, (d) => {
        d.text = `payload-${i}-distinct-content`;
      });
    }
    let authored = from<TextDoc>({ text: '' }, { author: AUTHOR });
    for (let i = 0; i < N; i++) {
      authored = change(authored, (d) => {
        d.text = `payload-${i}-distinct-content`;
      });
    }

    // The semantic pin: one footer total (the seq-1 change), not N.
    const footers = getAllChanges(authored).filter(
      (c) => decodeChange(c).author === AUTHOR,
    );
    expect(footers).toHaveLength(1);

    const before = save(anonymous).length;
    const after = save(authored).length;
    // Recorded in the plan's Phase 0 results.
    console.log(
      `author footer on-disk cost: ${N} changes, ` +
        `save() ${before} -> ${after} bytes ` +
        `(+${after - before} bytes total for one 34-byte footer)`,
    );

    // One footer is ~34 raw bytes (0x01 | leb128(32) | 32 bytes); the
    // delta must stay in that range no matter how many changes follow.
    expect(after).toBeGreaterThan(before);
    expect(after - before).toBeLessThan(128);
  });
});
