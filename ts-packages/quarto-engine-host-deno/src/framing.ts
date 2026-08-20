/**
 * @quarto/engine-host-deno — newline-framed JSON channel plumbing
 *
 * Implements the v1 wire protocol: one `Request` per newline-terminated UTF-8
 * JSON line in, one `Response` per newline-terminated UTF-8 JSON line out.
 *
 * Pure TypeScript — no `Deno.*` APIs. Uses only cross-runtime globals
 * (`ReadableStream`, `TextDecoder`, `TextEncoder`) so the module runs under
 * vitest/Node and Deno alike.
 *
 * Phase 1.6 (deferred): connectControl over loopback TCP
 */

import type { Request, Response } from "./types.js";

// ==================== FrameWriter ====================

/**
 * Minimal async byte-writer interface. Matches `Deno.stdout` (and anything
 * else that exposes `write(bytes): Promise<number>`). Trivially mockable in
 * Node-based tests without touching `Deno.*`.
 */
export interface FrameWriter {
  write(bytes: Uint8Array): Promise<number>;
}

// ==================== AsyncMutex ====================

/**
 * Tiny promise-chain serializer. Each `runExclusive` call chains onto a
 * private tail promise, guaranteeing that concurrent callers execute their
 * critical sections one-at-a-time in call order — with no overlap.
 *
 * Hand-rolled: no npm dependency. O(1) memory per queued caller.
 *
 * Usage pattern:
 *   const mutex = new AsyncMutex();
 *   await mutex.runExclusive(async () => { … });
 */
export class AsyncMutex {
  private tail: Promise<void> = Promise.resolve();

  runExclusive<T>(fn: () => Promise<T> | T): Promise<T> {
    // Chain: next runs only after the current tail settles.
    const next = this.tail.then(() => fn());
    // Advance the tail; swallow errors so a throwing fn never poisons the chain.
    this.tail = next.then(
      () => {},
      () => {}
    );
    return next;
  }
}

// ==================== Module-singleton write mutex ====================

/**
 * Module-singleton mutex for `writeFrame`. Design note: v1 has exactly one
 * protocol writer (stdout), so a single shared mutex is the simplest correct
 * choice. Attach a per-writer mutex if multiple concurrent writers are ever
 * needed.
 */
const writeMutex = new AsyncMutex();

/** Module-level encoder shared by all `writeFrame` calls — avoids per-call allocation. */
const enc = new TextEncoder();

// ==================== readFrames ====================

/**
 * Reads newline-framed UTF-8 JSON `Request` objects from a
 * `ReadableStream<Uint8Array>` (e.g. `Deno.stdin.readable`).
 *
 * Protocol properties:
 * - Decodes UTF-8 with `TextDecoder` in streaming mode so multi-byte
 *   characters split across chunk boundaries decode correctly.
 * - **Buffers partial lines across chunk boundaries** — a frame whose bytes
 *   span two or more chunks is fully reassembled before parsing. This is the
 *   load-bearing cross-chunk property (see named-revert test).
 * - Multiple complete frames in a single chunk each yield separately, in order.
 * - Empty lines (blank lines between frames) are silently skipped.
 * - The generator completes when the stream closes (EOF). Trailing partial
 *   lines that lack a terminating `\n` at EOF are dropped — the Rust host
 *   always terminates frames with `\n`, so a partial at EOF indicates a
 *   truncated or aborted session.
 * - If a line fails `JSON.parse`, the error propagates to the caller. Input
 *   comes from the trusted Rust host; defensive recovery is not warranted.
 */
export async function* readFrames(
  stream: ReadableStream<Uint8Array>
): AsyncGenerator<Request> {
  // streaming: true — keep multi-byte char state across chunks.
  // fatal:false — trusted UTF-8 from the Rust host; replace malformed bytes rather than throw.
  const decoder = new TextDecoder("utf-8", { fatal: false });
  // Cross-chunk accumulation buffer. Holds decoded text that has not yet
  // produced a complete (newline-terminated) line.
  let buffer = "";
  const reader = stream.getReader();

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) {
        // Flush the streaming TextDecoder (clears any buffered partial char).
        // We do NOT attempt to parse any remaining content in `buffer` because
        // a partial frame at EOF (no terminating \n) indicates session abort.
        decoder.decode(); // flush
        break;
      }
      // Decode chunk and append to accumulation buffer.
      buffer += decoder.decode(value, { stream: true });

      // Yield all complete lines (terminated by \n).
      let nl: number;
      while ((nl = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, nl);
        buffer = buffer.slice(nl + 1);
        if (line.length > 0) {
          yield JSON.parse(line) as Request;
        }
        // Empty line (line.length === 0): silently skip.
      }
    }
  } finally {
    reader.releaseLock();
  }
}

// ==================== writeFrame ====================

/**
 * Serializes `response` to JSON and writes it as a newline-terminated UTF-8
 * line to `out`.
 *
 * Implementation uses **two** awaited `write()` calls inside the mutex
 * (JSON payload bytes, then the newline byte). The two yield points inside
 * `runExclusive` are precisely what the `writeMutex` guards: without
 * serialization, concurrent callers interleave their JSON and newline writes,
 * producing malformed frames (e.g. `jsonA + jsonB + "\n" + "\n"`).
 *
 * Design note on single- vs. two-write: a single `encode(json + "\n")` call
 * would also be protocol-correct, but it makes the mutex property untestable
 * via byte-output inspection — JS single-threading prevents byte interleaving
 * from a single atomic write call. Two writes expose the serialization need in
 * tests and reflect the actual guard the mutex provides.
 *
 * Never uses a synchronous write — async `await out.write(…)` keeps the event
 * loop live so the read loop continues draining `stdin` while a large frame
 * goes out, preventing a full-pipe deadlock on large payloads.
 */
export function writeFrame(out: FrameWriter, response: Response): Promise<void> {
  return writeMutex.runExclusive(async () => {
    await out.write(enc.encode(JSON.stringify(response)));
    await out.write(enc.encode("\n"));
  });
}
