/**
 * Tests for src/framing.ts — newline-framed JSON channel plumbing.
 *
 * TDD: these tests were written before framing.ts existed. Each named-revert
 * test has a specific code change that makes it go RED.
 */
import { describe, it, expect } from "vitest";
import { readFrames, writeFrame, AsyncMutex } from "./framing.js";
import type { Request, Response } from "./types.js";

const enc = new TextEncoder();
const dec = new TextDecoder();

/** Build a ReadableStream<Uint8Array> that enqueues the given chunks in order. */
function makeStream(chunks: Uint8Array[]): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      for (const chunk of chunks) {
        controller.enqueue(chunk);
      }
      controller.close();
    },
  });
}

// ---------------------------------------------------------------------------
// readFrames
// ---------------------------------------------------------------------------

describe("readFrames", () => {
  it("multiple frames, one chunk", async () => {
    // Two newline-terminated JSON Request lines in a single chunk.
    const req1: Request = { id: 1, msg: { type: "shutdown" } };
    const req2: Request = { id: 2, msg: { type: "cancel", target: 1 } };
    const chunk = enc.encode(JSON.stringify(req1) + "\n" + JSON.stringify(req2) + "\n");
    const stream = makeStream([chunk]);

    const results: Request[] = [];
    for await (const r of readFrames(stream)) {
      results.push(r);
    }

    expect(results).toHaveLength(2);
    expect(results[0]).toEqual(req1);
    expect(results[1]).toEqual(req2);
  });

  it("cross-chunk reassembly", async () => {
    // Named revert: remove cross-chunk buffer accumulation (parse each chunk
    // independently) → this test goes RED because the partial JSON in chunk 1
    // fails to parse.
    const req: Request = { id: 42, msg: { type: "shutdown" } };
    const bytes = enc.encode(JSON.stringify(req) + "\n");
    // Split mid-JSON — chunk 1 contains no newline character.
    const mid = Math.floor(bytes.length / 2);
    const chunk1 = bytes.slice(0, mid);
    const chunk2 = bytes.slice(mid);
    const stream = makeStream([chunk1, chunk2]);

    const results: Request[] = [];
    for await (const r of readFrames(stream)) {
      results.push(r);
    }

    expect(results).toHaveLength(1);
    expect(results[0]).toEqual(req);
  });

  it("EOF terminates and flushes trailing complete line", async () => {
    // A stream whose last chunk ends with a complete frame followed by EOF.
    const req: Request = { id: 7, msg: { type: "shutdown" } };
    const chunk = enc.encode(JSON.stringify(req) + "\n");
    const stream = makeStream([chunk]);

    const results: Request[] = [];
    for await (const r of readFrames(stream)) {
      results.push(r);
    }

    expect(results).toHaveLength(1);
    expect(results[0]).toEqual(req);
  });

  it("empty lines skipped", async () => {
    // A chunk with "frame\n\nframe\n" yields exactly two frames, not three.
    const req1: Request = { id: 1, msg: { type: "shutdown" } };
    const req2: Request = { id: 2, msg: { type: "cancel", target: 1 } };
    const chunk = enc.encode(
      JSON.stringify(req1) + "\n\n" + JSON.stringify(req2) + "\n"
    );
    const stream = makeStream([chunk]);

    const results: Request[] = [];
    for await (const r of readFrames(stream)) {
      results.push(r);
    }

    expect(results).toHaveLength(2);
    expect(results[0]).toEqual(req1);
    expect(results[1]).toEqual(req2);
  });
});

// ---------------------------------------------------------------------------
// writeFrame
// ---------------------------------------------------------------------------

describe("writeFrame", () => {
  it("one newline-terminated JSON line", async () => {
    // Capture all written bytes; combined output must be JSON + exactly one "\n".
    const response: Response = { id: 1, msg: { type: "cancelled" } };
    const written: Uint8Array[] = [];
    const writer = {
      write(bytes: Uint8Array): Promise<number> {
        written.push(new Uint8Array(bytes));
        return Promise.resolve(bytes.length);
      },
    };

    await writeFrame(writer, response);

    // Combine all chunks written across potentially multiple write() calls.
    const totalLen = written.reduce((n, b) => n + b.length, 0);
    const combined = new Uint8Array(totalLen);
    let offset = 0;
    for (const b of written) {
      combined.set(b, offset);
      offset += b.length;
    }
    const decoded = dec.decode(combined);

    expect(decoded).toBe(JSON.stringify(response) + "\n");
  });

  it("concurrent writes do not interleave (the mutex property)", async () => {
    // Named revert: remove writeMutex.runExclusive() (write directly) → with this
    // yielding writer the two frames' bytes interleave → assertion RED.
    //
    // writeFrame performs TWO awaited writes (JSON payload + newline) inside the
    // mutex. Without the mutex, both frames start their JSON write, yield, both
    // start their newline write, yield — producing jsonA+jsonB+"\n"+"\n" instead
    // of jsonA+"\n"+jsonB+"\n".
    const respA: Response = { id: 1, msg: { type: "cancelled" } };
    const respB: Response = { id: 2, msg: { type: "error", message: "oops" } };

    const chunks: Uint8Array[] = [];
    const writer = {
      // Yields (Promise.resolve microtask) before recording bytes.
      write(bytes: Uint8Array): Promise<number> {
        return new Promise((resolve) => {
          Promise.resolve().then(() => {
            chunks.push(new Uint8Array(bytes));
            resolve(bytes.length);
          });
        });
      },
    };

    // Fire both concurrently — the mutex must serialize them.
    await Promise.all([writeFrame(writer, respA), writeFrame(writer, respB)]);

    const totalLen = chunks.reduce((n, b) => n + b.length, 0);
    const combined = new Uint8Array(totalLen);
    let off = 0;
    for (const b of chunks) {
      combined.set(b, off);
      off += b.length;
    }
    const decoded = dec.decode(combined);

    const frameA = JSON.stringify(respA) + "\n";
    const frameB = JSON.stringify(respB) + "\n";

    // With the mutex, frames are whole and back-to-back (A then B or B then A).
    // Without the mutex, the two writes per frame interleave: jsonA+jsonB+"\n\n".
    const isAB = decoded === frameA + frameB;
    const isBA = decoded === frameB + frameA;
    expect(isAB || isBA).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// AsyncMutex
// ---------------------------------------------------------------------------

describe("AsyncMutex", () => {
  it("ordered, non-overlapping execution", async () => {
    // Named revert: make runExclusive run fn immediately without chaining onto
    // the tail → the three tasks overlap → log order is wrong → RED.
    const mutex = new AsyncMutex();
    const log: string[] = [];

    const task = (n: number) =>
      mutex.runExclusive(async () => {
        log.push(`start${n}`);
        await Promise.resolve(); // yield to allow other tasks to start if mutex is absent
        log.push(`end${n}`);
      });

    // Launch all three concurrently.
    await Promise.all([task(1), task(2), task(3)]);

    // Perfect nesting: each task completes before the next starts.
    expect(log).toEqual(["start1", "end1", "start2", "end2", "start3", "end3"]);
  });
});
