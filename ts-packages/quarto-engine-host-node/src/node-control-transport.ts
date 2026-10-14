/**
 * @quarto/engine-host-node — control-channel dial-back over `node:net`.
 *
 * `connectControl` reads the
 * one-time token from the FIRST LINE of stdin, parses `--control
 * <host:port>` from argv, dials that loopback address, writes the token back
 * as the first bytes on the socket, and returns the `{ reader, writer }` pair
 * `runHost` expects.
 *
 * Stdin is read with `fs.readSync(0, …)` one byte at a time so no byte past
 * the `\n` is consumed (the Rust side's H-READER hazard) and so the lazy
 * `process.stdin` getter is never touched (it would set O_NONBLOCK on fd 0
 * and make later `readSync` calls fail with EAGAIN).
 */
import { readSync } from "node:fs";
import { connect, type Socket } from "node:net";
import process from "node:process";
import type { FrameWriter } from "./framing.js";

export interface ConnectControlDeps {
  /** Defaults to `process.argv.slice(2)`. */
  args?: string[];
  /** Reads up to `buf.length` bytes; returns 0 at EOF. Defaults to `readSync(0, …)`. */
  readStdin?: (buf: Uint8Array) => number;
}

/** Read from fd 0, retrying briefly on EAGAIN and treating Windows' EOF throw as EOF. */
function readStdinSync(buf: Uint8Array): number {
  for (;;) {
    try {
      return readSync(0, buf, 0, buf.length, null);
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code === "EAGAIN") {
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 5);
        continue;
      }
      // Windows reports EOF on a closed pipe as an error.
      if (code === "EOF") return 0;
      throw error;
    }
  }
}

/** Read the token line without consuming bytes past the first `\n`. */
function readTokenLine(read: (buf: Uint8Array) => number): string {
  const bytes: number[] = [];
  const buf = new Uint8Array(1);
  for (;;) {
    const n = read(buf);
    if (n === 0) break; // EOF: treat what we have as the whole token
    if (buf[0] === 0x0a) break;
    bytes.push(buf[0]);
  }
  return new TextDecoder("utf-8").decode(new Uint8Array(bytes));
}

/** Parse `--control <host:port>`; the LAST `:` separates host from port. */
function parseControlAddress(
  args: string[],
): { hostname: string; port: number } {
  const idx = args.indexOf("--control");
  if (idx === -1 || idx + 1 >= args.length) {
    throw new Error("connectControl: --control <host:port> not found in args");
  }
  const addr = args[idx + 1];
  const sep = addr.lastIndexOf(":");
  if (sep === -1) {
    throw new Error(`connectControl: malformed --control address: ${addr}`);
  }
  const hostname = addr.slice(0, sep);
  const port = Number(addr.slice(sep + 1));
  if (!Number.isInteger(port)) {
    throw new Error(
      `connectControl: malformed port in --control address: ${addr}`,
    );
  }
  return { hostname, port };
}

/**
 * Adapt a socket to the writer `runHost` uses: each `write` resolves when the
 * bytes were flushed to the kernel, and rejects if the socket errors first.
 */
function makeSocketWriter(socket: Socket): FrameWriter & { end(): Promise<void> } {
  let failure: Error | undefined;
  const pending = new Set<(error: Error) => void>();
  socket.on("error", (error) => {
    failure = error;
    for (const reject of pending) reject(error);
    pending.clear();
  });
  return {
    write(bytes: Uint8Array): Promise<number> {
      if (failure) return Promise.reject(failure);
      return new Promise<number>((resolve, reject) => {
        pending.add(reject);
        socket.write(bytes, (error) => {
          pending.delete(reject);
          if (error) reject(error);
          else resolve(bytes.length);
        });
      });
    },
    end(): Promise<void> {
      return new Promise((resolve) => {
        if (socket.destroyed) return resolve();
        socket.end(() => resolve());
      });
    },
  };
}

/** Expose the socket's chunks as a web `ReadableStream` (no `Readable.toWeb`). */
function socketReadable(socket: Socket): ReadableStream<Uint8Array> {
  const iterator = socket[Symbol.asyncIterator]();
  return new ReadableStream<Uint8Array>({
    async pull(controller) {
      const { done, value } = await iterator.next();
      if (done) controller.close();
      else controller.enqueue(new Uint8Array(value as Buffer));
    },
    cancel() {
      socket.destroy();
    },
  });
}

/**
 * Dial the control-channel address passed via `--control`, authenticate with
 * the token read from stdin, and return the pair `runHost` expects. The
 * returned writer also has `end()` to flush and close before exit.
 */
export async function connectControl(
  deps?: ConnectControlDeps,
): Promise<
  { reader: ReadableStream<Uint8Array>; writer: FrameWriter & { end(): Promise<void> } }
> {
  const args = deps?.args ?? process.argv.slice(2);
  const token = readTokenLine(deps?.readStdin ?? readStdinSync);
  const { hostname, port } = parseControlAddress(args);

  const socket = await new Promise<Socket>((resolve, reject) => {
    const s = connect({ host: hostname, port });
    s.once("connect", () => resolve(s));
    s.once("error", reject);
  });
  socket.setNoDelay(true);

  const writer = makeSocketWriter(socket);
  // The token is the FIRST bytes on the socket; the Rust `accept_and_handshake`
  // validates it before treating the connection as the protocol channel.
  await writer.write(new TextEncoder().encode(`${token}\n`));

  return { reader: socketReadable(socket), writer };
}
