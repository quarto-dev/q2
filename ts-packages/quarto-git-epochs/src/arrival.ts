/**
 * Arrival (G9, G14): what one file does after a checkout-type decision. Pure.
 * See the contract, "Arrival". Field and action names come from fixtures/decision-tables/arrival.json.
 */
import type { EventKind } from './vocabulary.js';

export interface ArrivalInput {
  peer: 'hub' | 'extension';
  /** Required for the hub; an extension row may carry it too (the `ReadOnly` variant). */
  policy?: 'ReadOnly' | 'WriteBack';
  kind: 'text' | 'binary';
  /** Disk content: CRLF-normalized text, or a byte hash. */
  disk: string;
  /** Canonical doc content (or its hash); absent when no doc is registered. */
  doc?: string;
  /** Blob content (or its hash) at the new HEAD. */
  blob: string;
  attached: 'none' | 'canonical' | 'other';
  retiredByOther?: boolean;
  jj?: boolean;
  canonicalUnavailable?: boolean;
  liveUnderOtherPath?: boolean;
  forkSyncs?: boolean;
  caseCollision?: boolean;
}

export type ArrivalStep = '1' | '2a' | '2b' | '3' | '4' | '5';

export interface ArrivalOutput {
  /** The step taken, or for a row outside the five steps the step it would otherwise take. */
  step: ArrivalStep;
  action: string;
  event?: EventKind;
  /** Discarded edits a retire step set aside (2b only). */
  discarded?: number;
}

/** Text compares after CRLF normalization (G9, `core.autocrlf`). */
export function normalizeText(s: string): string {
  return s.replace(/\r\n?/g, '\n');
}

export function arrive(input: ArrivalInput): ArrivalOutput {
  const hub = input.peer === 'hub';
  const out = baseStep(input, hub);
  const ev = (ext: EventKind, hubEv: EventKind): EventKind => (hub ? hubEv : ext);

  // A doc that another peer retired: stay in it, live (ordinary arrival at its next change).
  if (input.attached !== 'none' && input.retiredByOther) return { step: '2a', action: 'stay-live' };

  let result = out;

  // Colocated jj: files that would join or attach take the step 5 behavior. Step 1 has no doc, and 2a/2b are
  // about an already attached doc.
  if (input.jj && (result.step === '3' || result.step === '4' || result.step === '5')) {
    result = hub
      ? { step: '5', action: 'create-from-disk-unregistered', event: 'seeded' }
      : { step: '5', action: 'detach-and-ask', event: 'detached' };
  }

  // ReadOnly: attach only when disk equals the doc; otherwise create from disk. Never join, never detach.
  if (input.policy === 'ReadOnly' && (result.step === '4' || result.step === '5')) {
    result = { step: result.step, action: 'create-from-disk', event: 'seeded' };
  }

  // A doc live under another path is forked, never attached twice (G12).
  if (input.liveUnderOtherPath && ['attach-silently', 'join-with-notice', 'join-and-warn'].includes(result.action)) {
    result = {
      ...result,
      action: input.forkSyncs === false ? 'create-from-content-no-history' : 'fork-then-attach',
    };
  }

  if (input.canonicalUnavailable && result.step === '1') {
    result = { step: '1', action: 'wait-5s-then-create', event: ev('switched', 'flipped') };
  }

  if (input.caseCollision) return { step: result.step, action: 'ordinary-sync' };
  return result;
}

function baseStep(i: ArrivalInput, hub: boolean): ArrivalOutput {
  const ev = (ext: EventKind, hubEv: EventKind): EventKind => (hub ? hubEv : ext);
  const noDoc = i.doc === undefined || i.canonicalUnavailable === true;
  if (i.attached === 'canonical' && !noDoc) {
    if (i.disk === i.blob && i.doc !== i.disk) {
      return { step: '2b', action: 'retire-create-flip', event: 'retired', discarded: 1 };
    }
    return { step: '2a', action: 're-baseline' };
  }
  if (noDoc) return { step: '1', action: 'create-from-disk', event: ev('switched', 'flipped') };
  if (i.disk === i.doc) return { step: '3', action: 'attach-silently', event: ev('switched', 'flipped') };
  if (i.disk === i.blob) {
    return hub
      ? { step: '4', action: 'join-and-warn', event: 'joined' }
      : { step: '4', action: 'join-with-notice', event: 'switched' };
  }
  return hub
    ? { step: '5', action: 'create-from-disk-unregistered', event: 'seeded' }
    : { step: '5', action: 'detach-and-ask', event: 'detached' };
}
