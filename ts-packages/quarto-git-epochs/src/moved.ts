import { formatNotice, MOVED_ANONYMOUS, MOVED_NAMED } from './notices.js';

/**
 * The ephemeral message a peer sends on the old doc when it leaves because of a switch (G25). It has the shape of
 * the existing `leave` presence message and no reason field. Sending and showing it belong to the clients.
 */
export interface MovedMessage {
  type: 'moved';
  peerId: string;
}

/** The short text shown for a moved message. */
export function movedNotice(name?: string): string {
  return name ? formatNotice(MOVED_NAMED, { name }) : MOVED_ANONYMOUS;
}
