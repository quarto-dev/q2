/**
 * A small request/response RPC over a `postMessage` endpoint (a Worker,
 * a worker's global scope, or a MessagePort). Each side registers the
 * methods it answers and can call the other's.
 *
 * Errors cross the boundary as `{ name, message, stack }` and are rebuilt
 * as `Error`s with the same `name`, so callers can keep matching on it
 * (`RenderSupersededError`, `RasterizerUnavailable`, `AbortError`).
 *
 * `serial: true` handles incoming requests one at a time, in arrival
 * order: a request starts only after the previous one settled. The render
 * worker uses it so a VFS write that arrives during a render is applied
 * after that render and before the next (preview VFS contract D9).
 * Responses to this side's own outbound calls are never queued, so a
 * handler may call the other side and await the answer.
 */

/** The `postMessage` surface both sides need. */
export interface Endpoint {
  postMessage(message: unknown, transfer?: Transferable[]): void;
  addEventListener(type: 'message', listener: (event: MessageEvent) => void): void;
  start?: () => void;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type Handlers = Record<string, ((...args: any[]) => unknown) | undefined>;

interface RequestMessage {
  rpc: 'req';
  id: number;
  method: string;
  args: unknown[];
}

interface ResponseMessage {
  rpc: 'res';
  id: number;
  ok: boolean;
  value?: unknown;
  error?: { name: string; message: string; stack?: string };
}

/** The value a handler returns plus the buffers to transfer with it. */
export class Transfer<T> {
  readonly value: T;
  readonly transfer: Transferable[];

  constructor(value: T, transfer: Transferable[]) {
    this.value = value;
    this.transfer = transfer;
  }
}

export class Rpc {
  private nextId = 1;
  private readonly pending = new Map<
    number,
    { resolve: (value: unknown) => void; reject: (reason: unknown) => void }
  >();
  private queue: Promise<void> = Promise.resolve();
  private readonly endpoint: Endpoint;
  private readonly handlers: Handlers;
  private readonly options: { serial?: boolean };

  constructor(endpoint: Endpoint, handlers: Handlers, options: { serial?: boolean } = {}) {
    this.endpoint = endpoint;
    this.handlers = handlers;
    this.options = options;
    endpoint.addEventListener('message', (event) => this.receive(event.data));
    endpoint.start?.();
  }

  /** Call `method` on the other side. */
  call<T = unknown>(method: string, args: unknown[], transfer: Transferable[] = []): Promise<T> {
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (value: unknown) => void, reject });
      const message: RequestMessage = { rpc: 'req', id, method, args };
      this.endpoint.postMessage(message, transfer);
    });
  }

  private receive(data: unknown): void {
    if (!data || typeof data !== 'object' || !('rpc' in data)) return;
    const message = data as RequestMessage | ResponseMessage;
    if (message.rpc === 'res') {
      const waiter = this.pending.get(message.id);
      if (!waiter) return;
      this.pending.delete(message.id);
      if (message.ok) {
        waiter.resolve(message.value);
      } else {
        const error = new Error(message.error?.message ?? 'remote call failed');
        error.name = message.error?.name ?? 'Error';
        if (message.error?.stack) error.stack = message.error.stack;
        waiter.reject(error);
      }
      return;
    }
    if (this.options.serial) {
      this.queue = this.queue.then(() => this.handle(message));
    } else {
      void this.handle(message);
    }
  }

  private async handle(request: RequestMessage): Promise<void> {
    let response: ResponseMessage;
    let transfer: Transferable[] = [];
    try {
      const handler = this.handlers[request.method];
      if (!handler) throw new Error(`no handler for "${request.method}"`);
      let value = await handler(...request.args);
      if (value instanceof Transfer) {
        transfer = value.transfer;
        value = value.value;
      }
      response = { rpc: 'res', id: request.id, ok: true, value };
    } catch (err) {
      const error =
        err instanceof Error
          ? { name: err.name, message: err.message, stack: err.stack }
          : { name: 'Error', message: String(err) };
      response = { rpc: 'res', id: request.id, ok: false, error };
    }
    this.endpoint.postMessage(response, transfer);
  }
}
