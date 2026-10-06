/**
 * Cursor presence over Automerge ephemeral messages. Same wire shape as
 * hub-client/src/services/presenceService.ts so cursors interoperate with
 * the web client both ways. Positions travel as Automerge cursor tokens on
 * `['text']`, so they stay put under concurrent edits.
 */
import { next as A } from '@automerge/automerge/slim';
import type { DocHandle } from '@automerge/automerge-repo/slim';

export interface Identity {
  userId: string;
  userName: string;
  userColor: string;
}

interface PresenceMessage extends Identity {
  type: 'presence';
  peerId: string;
  cursor: string | null;
  selection: { start: string; end: string } | null;
}
type Message = PresenceMessage | { type: 'leave'; peerId: string };

export interface RemoteCursor extends Identity {
  peerId: string;
  cursor: number | null;
  selection: { start: number; end: number } | null;
}

type TextDoc = { text: string };

export class Presence {
  private readonly peerId = crypto.randomUUID();
  private readonly remote = new Map<string, { msg: PresenceMessage; seen: number }>();
  private readonly timer: ReturnType<typeof setInterval>;

  constructor(
    private readonly handle: DocHandle<TextDoc>,
    private readonly me: Identity,
    private readonly onChange: (cursors: RemoteCursor[]) => void,
  ) {
    handle.on('ephemeral-message', this.receive);
    handle.on('change', this.notify); // tokens resolve to new offsets after edits
    this.timer = setInterval(() => {
      const now = Date.now();
      for (const [id, p] of this.remote) if (now - p.seen > 5000) this.remote.delete(id);
      this.notify();
    }, 2000);
  }

  /** Broadcast our cursor (UTF-16 offsets). */
  update(cursor: number, selection: { start: number; end: number } | null): void {
    const doc = this.handle.doc();
    if (!doc) return;
    const tok = (o: number) => A.getCursor(doc, ['text'], o);
    const msg: PresenceMessage = {
      type: 'presence',
      peerId: this.peerId,
      ...this.me,
      cursor: tok(cursor),
      selection: selection && { start: tok(selection.start), end: tok(selection.end) },
    };
    this.handle.broadcast(msg);
  }

  dispose(): void {
    clearInterval(this.timer);
    this.handle.off('ephemeral-message', this.receive);
    this.handle.off('change', this.notify);
    try {
      this.handle.broadcast({ type: 'leave', peerId: this.peerId } satisfies Message);
    } catch {
      /* socket may be gone */
    }
  }

  private receive = ({ message }: { message: unknown }) => {
    const m = message as Message;
    if (!m || typeof m !== 'object' || m.peerId === this.peerId) return;
    if (m.type === 'leave') this.remote.delete(m.peerId);
    else if (m.type === 'presence') this.remote.set(m.peerId, { msg: m, seen: Date.now() });
    this.notify();
  };

  private notify = () => {
    const doc = this.handle.doc();
    const pos = (tok: string | null) => {
      if (!doc || tok === null) return null;
      try {
        return A.getCursorPosition(doc, ['text'], tok);
      } catch {
        return null;
      }
    };
    this.onChange(
      [...this.remote.values()].map(({ msg: m }) => {
        const start = pos(m.selection?.start ?? null);
        const end = pos(m.selection?.end ?? null);
        return {
          peerId: m.peerId,
          userId: m.userId,
          userName: m.userName,
          userColor: m.userColor,
          cursor: pos(m.cursor),
          selection: start !== null && end !== null ? { start, end } : null,
        };
      }),
    );
  };
}
