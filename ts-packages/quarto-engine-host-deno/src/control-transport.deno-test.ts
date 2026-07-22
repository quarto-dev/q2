/**
 * Deno-native test for `connectControl` (D-CONNECT) — seam #8 of
 * plan1a.6 Phase 2 ("Deno dial-back").
 *
 * Run from the repo root:
 *   deno test --allow-all ts-packages/quarto-engine-host-deno/src/control-transport.deno-test.ts
 *
 * This file is excluded from the tsc/vitest graph:
 * - tsconfig.json: exclude "src/**\/*.deno-test.ts"
 * - vitest.config.ts: exclude "**\/*.deno-test.ts"
 *
 * Stands up a REAL `Deno.listen({ port: 0 })` loopback socket and drives
 * `connectControl` against it with a fake stdin — no mock `Deno.Conn`.
 *
 * Fail-on-revert bindings (state as required by the task brief):
 * - revert D-CONNECT `writeAll(token+"\n")` (don't write the token pre-line)
 *   → the "first bytes on the socket == token" assertion goes RED.
 * - revert D-CONNECT `{ reader: conn.readable }` (return an empty/fresh
 *   reader instead of the real `conn.readable`) → the round-trip assertion
 *   goes RED.
 */
import { assertEquals } from "jsr:@std/assert";
import { connectControl } from "./control-transport.ts";
import { readFrames, writeFrame } from "./framing.ts";
import type { FrameWriter } from "./framing.ts";
import type { Request, Response } from "./types.ts";

/** A fake stdin that yields `text`'s bytes once, then EOF (`null`). */
function fakeStdin(text: string): { read(p: Uint8Array): Promise<number | null> } {
  const bytes = new TextEncoder().encode(text);
  let offset = 0;
  return {
    read(p: Uint8Array): Promise<number | null> {
      if (offset >= bytes.length) {
        return Promise.resolve(null);
      }
      const n = Math.min(p.length, bytes.length - offset);
      p.set(bytes.subarray(offset, offset + n));
      offset += n;
      return Promise.resolve(n);
    },
  };
}

/**
 * Races `promise` against a rejection after `ms` milliseconds. Used so a
 * D-CONNECT revert (which withholds bytes the assertion is waiting to read)
 * fails FAST with a clear error, instead of hanging the test runner forever.
 */
function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
  return Promise.race([
    promise,
    new Promise<T>((_, reject) => {
      setTimeout(() => reject(new Error(`timed out after ${ms}ms waiting for: ${label}`)), ms);
    }),
  ]);
}

Deno.test("connectControl - writes token pre-line then round-trips a frame", async () => {
  const token = "test-token-abc123";

  const listener = Deno.listen({ hostname: "127.0.0.1", port: 0, transport: "tcp" });
  const addr = listener.addr as Deno.NetAddr;
  const port = addr.port;

  try {
    // Drive connectControl with a fake stdin (token + "\n") and --control
    // pointing at the just-bound ephemeral port.
    const clientPromise = connectControl({
      args: ["--control", `127.0.0.1:${port}`],
      stdin: fakeStdin(`${token}\n`),
    });

    // Accept the client's connection server-side.
    const serverConn = await listener.accept();
    try {
      // ── Assertion 1 (FIRST, order-checked): the socket pre-line equals
      // the token. Read raw bytes off the accepted conn ourselves (not via
      // readFrames, so this check is independent of the framing module).
      const first = await withTimeout(
        readFirstLine(serverConn),
        3000,
        "first line on the socket",
      );
      assertEquals(first, token, "first bytes on the socket must equal token+\\n");

      const control = await clientPromise;

      // ── Assertion 2: round-trip a Request-shaped frame server → client.
      // writeFrame is typed for Response; the wire format is newline-JSON
      // either way (both are `{ id, msg }` envelopes), so cast the test frame.
      const serverWriter: FrameWriter = {
        write: (bytes) => serverConn.write(bytes),
      };
      const frame: Request = { id: 1, msg: { type: "shutdown" } };
      await writeFrame(serverWriter, frame as unknown as Response);

      const received = await withTimeout(
        firstFrame(control.reader),
        3000,
        "round-tripped frame",
      );
      assertEquals(received, frame, "round-tripped frame must deep-equal what was sent");
    } finally {
      serverConn.close();
    }
  } finally {
    listener.close();
  }
});

/** Read bytes off `conn` up to and including the first `\n`; return the line (without `\n`). */
async function readFirstLine(conn: Deno.Conn): Promise<string> {
  const reader = conn.readable.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) throw new Error("EOF before newline");
      buffer += decoder.decode(value, { stream: true });
      const nl = buffer.indexOf("\n");
      if (nl !== -1) {
        return buffer.slice(0, nl);
      }
    }
  } finally {
    reader.releaseLock();
  }
}

/** Read exactly one frame off `reader` via the real `readFrames` generator. */
async function firstFrame(reader: ReadableStream<Uint8Array>) {
  for await (const frame of readFrames(reader)) {
    return frame;
  }
  throw new Error("stream closed before a frame arrived");
}
