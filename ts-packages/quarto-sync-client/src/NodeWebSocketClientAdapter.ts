/**
 * Node-only WebSocket client adapter with `Authorization` header support.
 *
 * The upstream `BrowserWebSocketClientAdapter` constructs its socket as
 * `new WebSocket(url)` and does not thread custom headers through —
 * which makes it unusable for Bearer-authenticated WS upgrades. This
 * adapter mirrors the upstream wire protocol exactly but constructs
 * `new WebSocket(url, [], { headers })` against the Node-native `ws`
 * package so an `Authorization: Bearer <token>` header is attached to
 * the upgrade request.
 *
 * Auth tokens are pulled via the caller-supplied `getBearer()` getter
 * on every `connect` (including upstream's retry path), so the retry
 * loop always sees the freshest token.
 *
 * Tokens never appear in this module's logs: the `redactAuthorization`
 * helper strips header dumps before logging, and the file does no
 * direct token logging.
 */

import { NetworkAdapter } from '@automerge/automerge-repo/slim';
import { cbor } from '@automerge/automerge-repo/slim';
import type {
  Message,
  PeerId,
  PeerMetadata,
} from '@automerge/automerge-repo/slim';
// `ws` is intentionally direct — `isomorphic-ws` Node entry resolves
// to the same module, but the headers option is not portable to a
// browser `WebSocket`. This adapter is Node-only by design.
import WebSocket from 'ws';

const ProtocolV1 = '1';
const READY_TIMEOUT_MS = 1000;
const WS_OPEN = 1;

type WSEventHandler = (event: unknown) => void;

interface ServerMessage {
  readonly type?: string;
  readonly senderId?: string;
  readonly peerMetadata?: PeerMetadata;
}

/**
 * The subset of the WebSocket interface this adapter actually uses.
 * Lets tests provide a tiny fake without standing up a real connection.
 */
export interface WebSocketLike {
  readyState: number;
  binaryType: string;
  addEventListener(
    type: 'open' | 'close' | 'message' | 'error',
    handler: WSEventHandler,
  ): void;
  removeEventListener(
    type: 'open' | 'close' | 'message' | 'error',
    handler: WSEventHandler,
  ): void;
  close(): void;
  send(data: Uint8Array): void;
}

/**
 * Test seam: callers can inject a WebSocket factory so unit tests don't
 * require a real server. Defaults to the `ws` constructor.
 */
export type WebSocketFactory = (
  url: string,
  protocols: readonly string[],
  options: { readonly headers: Record<string, string> },
) => WebSocketLike;

export interface NodeWebSocketClientAdapterOptions {
  /**
   * Async getter for the Bearer token. Called once per socket open,
   * including upstream's retry path — so a refresh during reconnect
   * is picked up on the next attempt.
   */
  readonly getBearer: () => Promise<string>;
  /** Retry interval in ms; 0 disables retry. Defaults to 5000 (matches upstream). */
  readonly retryInterval?: number;
  /** Test hook — defaults to the `ws` constructor. */
  readonly webSocketFactory?: WebSocketFactory;
}

/**
 * Replace any `Authorization: Bearer …` value in an arbitrary string
 * with the literal `Authorization: [redacted]`. Used before logging
 * error details so a token never reaches stderr / logs even via the
 * underlying library.
 */
export function redactAuthorization(s: string): string {
  return s.replace(
    /(Authorization\s*[:=]\s*)Bearer\s+[A-Za-z0-9._\-+/=]+/gi,
    '$1[redacted]',
  );
}

const defaultWebSocketFactory: WebSocketFactory = (url, protocols, options) =>
  new WebSocket(url, [...protocols], options) as unknown as WebSocketLike;

const cborApi = cbor as {
  encode(value: unknown): Uint8Array;
  decode(bytes: Uint8Array): unknown;
};

export class NodeWebSocketClientAdapter extends NetworkAdapter {
  readonly url: string;
  readonly retryInterval: number;
  private readonly getBearer: () => Promise<string>;
  private readonly wsFactory: WebSocketFactory;

  private socket: WebSocketLike | undefined;
  private retryIntervalId: ReturnType<typeof setInterval> | undefined;
  private ready = false;
  private readyResolver?: () => void;
  private readonly readyPromise: Promise<void>;
  private remotePeerId: PeerId | undefined;

  constructor(url: string, opts: NodeWebSocketClientAdapterOptions) {
    super();
    this.url = url;
    this.retryInterval = opts.retryInterval ?? 5000;
    this.getBearer = opts.getBearer;
    this.wsFactory = opts.webSocketFactory ?? defaultWebSocketFactory;
    this.readyPromise = new Promise<void>((resolve) => {
      this.readyResolver = resolve;
    });
  }

  isReady(): boolean {
    return this.ready;
  }

  whenReady(): Promise<void> {
    return this.readyPromise;
  }

  private forceReady(): void {
    if (!this.ready) {
      this.ready = true;
      this.readyResolver?.();
    }
  }

  connect(peerId: PeerId, peerMetadata?: PeerMetadata): void {
    if (!this.socket || !this.peerId) {
      this.peerId = peerId;
      this.peerMetadata = peerMetadata ?? {};
    } else {
      this.removeListeners(this.socket);
    }

    if (!this.retryIntervalId && this.retryInterval > 0) {
      this.retryIntervalId = setInterval(() => {
        this.connect(peerId, peerMetadata);
      }, this.retryInterval);
    }

    // The token fetch is async, but upstream's contract for `connect`
    // is synchronous. Kick off the fetch and wire the socket up when
    // it resolves; any pre-socket failure leaves the socket unset and
    // the retry interval will try again.
    void this.openSocket();

    // Mark adapter ready after 1 s regardless, so Repo doesn't stall
    // forever waiting on the handshake.
    setTimeout(() => this.forceReady(), READY_TIMEOUT_MS);
  }

  private async openSocket(): Promise<void> {
    let token: string;
    try {
      token = await this.getBearer();
    } catch {
      // Token-fetch failure: leave the socket unset. The retry loop
      // will try again, and the underlying refresh manager (if any)
      // surfaces the error to its own caller.
      return;
    }

    const socket = this.wsFactory(this.url, [], {
      headers: { Authorization: `Bearer ${token}` },
    });
    socket.binaryType = 'arraybuffer';
    socket.addEventListener('open', this.onOpen);
    socket.addEventListener('close', this.onClose);
    socket.addEventListener('message', this.onMessage);
    socket.addEventListener('error', this.onError);
    this.socket = socket;
    this.join();
  }

  private readonly onOpen = (): void => {
    if (this.retryIntervalId) {
      clearInterval(this.retryIntervalId);
      this.retryIntervalId = undefined;
    }
    this.join();
  };

  private readonly onClose = (): void => {
    if (this.remotePeerId) {
      this.emit('peer-disconnected', { peerId: this.remotePeerId });
    }
    if (this.retryInterval > 0 && !this.retryIntervalId && this.peerId) {
      const peerId = this.peerId;
      const peerMetadata = this.peerMetadata;
      setTimeout(() => this.connect(peerId, peerMetadata), this.retryInterval);
    }
  };

  private readonly onMessage = (event: unknown): void => {
    const data = (event as { data: ArrayBuffer | Uint8Array }).data;
    if (data instanceof ArrayBuffer) {
      this.receiveMessage(new Uint8Array(data));
    } else if (data instanceof Uint8Array) {
      this.receiveMessage(data);
    }
  };

  private readonly onError = (event: unknown): void => {
    const ev = event as { error?: { code?: string; message?: string } };
    if (ev.error && ev.error.code !== 'ECONNREFUSED') {
      // Re-throw with redacted message so an Authorization-bearing
      // string (defensive — `ws` doesn't currently put one here) does
      // not propagate to user log sinks.
      throw new Error(redactAuthorization(ev.error.message ?? 'WebSocket error'));
    }
  };

  private removeListeners(socket: WebSocketLike): void {
    socket.removeEventListener('open', this.onOpen);
    socket.removeEventListener('close', this.onClose);
    socket.removeEventListener('message', this.onMessage);
    socket.removeEventListener('error', this.onError);
  }

  private join(): void {
    if (!this.peerId || !this.socket) return;
    if (this.socket.readyState === WS_OPEN) {
      this.sendRaw({
        type: 'join',
        senderId: this.peerId,
        peerMetadata: this.peerMetadata ?? {},
        supportedProtocolVersions: [ProtocolV1],
      });
    }
  }

  disconnect(): void {
    if (this.socket) {
      this.removeListeners(this.socket);
      this.socket.close();
    }
    if (this.retryIntervalId) {
      clearInterval(this.retryIntervalId);
      this.retryIntervalId = undefined;
    }
    if (this.remotePeerId) {
      this.emit('peer-disconnected', { peerId: this.remotePeerId });
    }
    this.socket = undefined;
  }

  send(message: Message): void {
    if (!this.peerId || !this.socket) return;
    const m = message as { data?: { byteLength: number } };
    if (m.data && m.data.byteLength === 0) {
      throw new Error('Tried to send a zero-length message');
    }
    this.sendRaw(message);
  }

  private sendRaw(message: unknown): void {
    if (!this.socket) return;
    if (this.socket.readyState !== WS_OPEN) {
      throw new Error(`Websocket not ready (${this.socket.readyState})`);
    }
    // The Node `ws` package and the browser `WebSocket` both accept
    // a `Uint8Array` directly, so we can hand off cbor's output without
    // allocating a fresh `ArrayBuffer` per message.
    this.socket.send(cborApi.encode(message));
  }

  private peerCandidate(remotePeerId: PeerId, peerMetadata: PeerMetadata): void {
    this.forceReady();
    this.remotePeerId = remotePeerId;
    this.emit('peer-candidate', { peerId: remotePeerId, peerMetadata });
  }

  private receiveMessage(messageBytes: Uint8Array): void {
    if (messageBytes.byteLength === 0) {
      throw new Error('received a zero-length message');
    }
    let message: ServerMessage;
    try {
      message = cborApi.decode(messageBytes) as ServerMessage;
    } catch {
      return;
    }
    if (message.type === 'peer' && message.senderId) {
      this.peerCandidate(
        message.senderId as PeerId,
        message.peerMetadata ?? {},
      );
    } else if (message.type === 'error') {
      // Wire-level errors don't carry tokens; drop without logging.
    } else {
      this.emit('message', message as unknown as Message);
    }
  }
}
