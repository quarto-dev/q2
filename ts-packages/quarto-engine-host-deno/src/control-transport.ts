/**
 * @quarto/engine-host-deno — control-channel dial-back (D-CONNECT)
 *
 * Deno-only module (references `Deno.*`) — excluded from tsconfig.json and
 * the vitest test runner, exactly like `deno-host.ts`/`main.ts`.
 *
 * Plan: claude-notes/plans/2026-07-08-plan1a6-off-stdout-loopback-tcp.md
 * ("Deno side" section). Phase 1 (Rust) built the TCP transport side;
 * this is Phase 2, the Deno harness learning to dial back.
 *
 * `connectControl` reads a one-time token from the FIRST LINE of stdin
 * (line-bounded — never consuming bytes past the `\n`, the Deno-side
 * analogue of the Rust H-READER byte-loss hazard), parses `--control
 * <host:port>` out of argv, dials that address over TCP, writes the token
 * back as the first bytes on the socket (the pre-line
 * `accept_and_handshake` on the Rust side validates), and returns a
 * `{ reader, writer }` pair that plugs directly into the existing
 * `runHost(reader, writer, host)` — no signature change needed there.
 */
import type { FrameWriter } from "./framing.ts";

export interface ConnectControlDeps {
  /** Defaults to `Deno.args`. */
  args?: string[];
  /** Defaults to `Deno.stdin`. */
  stdin?: { read(p: Uint8Array): Promise<number | null> };
}

/**
 * Reads the control token from the first line of `stdin`, one byte at a
 * time (startup cost is negligible; a chunked read could swallow bytes
 * written after the `\n`, which the Deno-side H-READER discipline forbids).
 * Stops at (and does not consume past) the first `\n`.
 */
async function readTokenLine(stdin: {
  read(p: Uint8Array): Promise<number | null>;
}): Promise<string> {
  const bytes: number[] = [];
  const buf = new Uint8Array(1);
  while (true) {
    const n = await stdin.read(buf);
    if (n === null || n === 0) {
      // EOF before a newline — no more token to read. Treat whatever was
      // accumulated as the whole token (best-effort; a well-formed caller
      // always terminates the token with "\n").
      break;
    }
    if (buf[0] === 0x0a /* "\n" */) {
      break;
    }
    bytes.push(buf[0]);
  }
  return new TextDecoder("utf-8").decode(new Uint8Array(bytes));
}

/**
 * Parses `--control <host:port>` out of `args`. The address's LAST `:`
 * separates hostname from the numeric port (mirrors the Rust side's
 * `127.0.0.1:<port>` argv form).
 */
function parseControlAddress(args: string[]): { hostname: string; port: number } {
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
    throw new Error(`connectControl: malformed port in --control address: ${addr}`);
  }
  return { hostname, port };
}

/**
 * Wraps a `Deno.Conn` in a `FrameWriter` whose `write` is short-write-safe:
 * loops until every byte has been accepted by the socket. This matters
 * because `writeFrame` (framing.ts) does a SINGLE `out.write(bytes)` call
 * with no retry loop — over a real socket (unlike the pipe-backed
 * `Deno.stdout` write in the stdio branch) a short write is possible, and
 * without this loop a frame could be truncated on the wire.
 */
function makeSocketWriter(conn: Deno.Conn): FrameWriter {
  return {
    async write(bytes: Uint8Array): Promise<number> {
      let n = 0;
      while (n < bytes.length) {
        n += await conn.write(bytes.subarray(n));
      }
      return bytes.length;
    },
  };
}

/**
 * Dials the control-channel address passed via `--control`, authenticates
 * with the token read from stdin, and returns the `{ reader, writer }` pair
 * `runHost` expects.
 */
export async function connectControl(
  deps?: ConnectControlDeps,
): Promise<{ reader: ReadableStream<Uint8Array>; writer: FrameWriter }> {
  const args = deps?.args ?? Deno.args;
  const stdin = deps?.stdin ?? Deno.stdin;

  const token = await readTokenLine(stdin);
  const { hostname, port } = parseControlAddress(args);

  const conn = await Deno.connect({ hostname, port, transport: "tcp" });
  conn.setNoDelay(true);

  const writer = makeSocketWriter(conn);

  // Write the token as the FIRST bytes on the socket — the pre-line
  // `accept_and_handshake` (Rust side) validates before treating the
  // connection as the protocol channel.
  await writer.write(new TextEncoder().encode(`${token}\n`));

  return { reader: conn.readable, writer };
}
