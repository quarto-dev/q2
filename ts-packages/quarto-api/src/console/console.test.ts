/**
 * @quarto/api — console namespace tests (HOST-ONLY)
 *
 * Seam-spec binding: assertions are on the namespace's ROUTING+FORMAT, not on
 * the fake's return value. A spy asserting "was it called with the right args
 * at the right level" directly tests the dispatch logic.
 *
 * Named reverts that reden each test are noted inline.
 *
 * No Deno.* / node:* anywhere.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { makeConsole } from "./index.js";

// ─── Fake host builder ────────────────────────────────────────────────────────

function makeFakeHost() {
  return {
    log: {
      info: vi.fn<(msg: string) => void>(),
      warning: vi.fn<(msg: string) => void>(),
      error: vi.fn<(msg: string) => void>(),
      clearLine: vi.fn<() => void>(),
    },
  };
}

// ─── console.info ─────────────────────────────────────────────────────────────

describe("console.info", () => {
  let host: ReturnType<typeof makeFakeHost>;
  let cons: ReturnType<typeof makeConsole>;

  beforeEach(() => {
    host = makeFakeHost();
    cons = makeConsole(host);
  });

  it("routes to host.log.info (count ≥ 1)", () => {
    // Revert routing to host.log.error → RED (wrong spy called)
    cons.info("hello info");
    expect(host.log.info).toHaveBeenCalledTimes(1);
  });

  it("passes the message to host.log.info", () => {
    // Revert to passing empty string → RED
    cons.info("hello info");
    expect(host.log.info).toHaveBeenCalledWith("hello info");
  });

  it("does NOT route to warning or error", () => {
    cons.info("info msg");
    expect(host.log.warning).not.toHaveBeenCalled();
    expect(host.log.error).not.toHaveBeenCalled();
  });
});

// ─── console.warning ──────────────────────────────────────────────────────────

describe("console.warning", () => {
  let host: ReturnType<typeof makeFakeHost>;
  let cons: ReturnType<typeof makeConsole>;

  beforeEach(() => {
    host = makeFakeHost();
    cons = makeConsole(host);
  });

  it("routes to host.log.warning (count ≥ 1)", () => {
    // Revert routing to host.log.info → RED
    cons.warning("watch out");
    expect(host.log.warning).toHaveBeenCalledTimes(1);
  });

  it("passes the message to host.log.warning", () => {
    cons.warning("watch out");
    expect(host.log.warning).toHaveBeenCalledWith("watch out");
  });

  it("does NOT route to info or error", () => {
    cons.warning("warn msg");
    expect(host.log.info).not.toHaveBeenCalled();
    expect(host.log.error).not.toHaveBeenCalled();
  });
});

// ─── console.error ────────────────────────────────────────────────────────────

describe("console.error", () => {
  let host: ReturnType<typeof makeFakeHost>;
  let cons: ReturnType<typeof makeConsole>;

  beforeEach(() => {
    host = makeFakeHost();
    cons = makeConsole(host);
  });

  it("routes to host.log.error (count ≥ 1)", () => {
    // Revert routing to host.log.info → RED
    cons.error("boom");
    expect(host.log.error).toHaveBeenCalledTimes(1);
  });

  it("passes the message to host.log.error", () => {
    cons.error("boom");
    expect(host.log.error).toHaveBeenCalledWith("boom");
  });

  it("does NOT route to info or warning", () => {
    cons.error("err msg");
    expect(host.log.info).not.toHaveBeenCalled();
    expect(host.log.warning).not.toHaveBeenCalled();
  });
});

// ─── console.completeMessage ──────────────────────────────────────────────────

describe("console.completeMessage", () => {
  let host: ReturnType<typeof makeFakeHost>;
  let cons: ReturnType<typeof makeConsole>;

  beforeEach(() => {
    host = makeFakeHost();
    cons = makeConsole(host);
  });

  it("routes to host.log.info (count ≥ 1)", () => {
    // Revert routing to a no-op → RED
    cons.completeMessage("done");
    expect(host.log.info).toHaveBeenCalledTimes(1);
  });

  it("includes the message text in the call", () => {
    // Revert to calling info with empty string → RED
    cons.completeMessage("done");
    const call = host.log.info.mock.calls[0][0];
    expect(call).toContain("done");
  });

  it("formats the completion with the Q1 bracket-check prefix", () => {
    // Revert format to plain message (no prefix) → RED
    // Q1 format: "[✓] msg"
    cons.completeMessage("Render complete");
    const call = host.log.info.mock.calls[0][0];
    expect(call).toMatch(/\[.+\]/); // bracket-enclosed marker present
    expect(call).toContain("Render complete");
  });
});

// ─── console.withSpinner ──────────────────────────────────────────────────────

describe("console.withSpinner", () => {
  let host: ReturnType<typeof makeFakeHost>;
  let cons: ReturnType<typeof makeConsole>;

  beforeEach(() => {
    host = makeFakeHost();
    cons = makeConsole(host);
  });

  it("returns the wrapped fn's result", async () => {
    // Revert to returning undefined → RED
    const result = await cons.withSpinner(
      { message: "working" },
      () => Promise.resolve(42),
    );
    expect(result).toBe(42);
  });

  it("emits start message to host.log.info (count ≥ 1)", async () => {
    // Revert to not calling host.log at all → RED
    await cons.withSpinner({ message: "loading..." }, () =>
      Promise.resolve("done"),
    );
    // At minimum the start message reaches host.log.info
    expect(host.log.info).toHaveBeenCalled();
    const calls = host.log.info.mock.calls.map((c) => c[0] as string);
    expect(calls.some((msg) => msg.includes("loading..."))).toBe(true);
  });

  it("emits completion message to host.log.info after fn completes", async () => {
    // Revert to not emitting completion → RED
    await cons.withSpinner({ message: "working" }, () =>
      Promise.resolve(null),
    );
    // At least two calls: start + completion
    expect(host.log.info.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it("uses doneMessage string when provided", async () => {
    // Revert to ignoring doneMessage → RED
    await cons.withSpinner(
      { message: "working", doneMessage: "All done!" },
      () => Promise.resolve(null),
    );
    const calls = host.log.info.mock.calls.map((c) => c[0] as string);
    expect(calls.some((msg) => msg.includes("All done!"))).toBe(true);
  });

  it("suppresses completion message when doneMessage === false", async () => {
    // Revert to always emitting completion → some tests may pass but this is
    // the Q1 contract: false means no done output.
    await cons.withSpinner(
      { message: "silent", doneMessage: false },
      () => Promise.resolve(null),
    );
    // Only the start message; no completion call to completeMessage
    // (which itself calls info). Count varies by impl; key: no bracket-check marker.
    const calls = host.log.info.mock.calls.map((c) => c[0] as string);
    const hasCompletion = calls.some((msg) => msg.includes("[") && msg.includes("]"));
    expect(hasCompletion).toBe(false);
  });

  it("resolves the start message thunk when message is a function", async () => {
    // Revert to not calling the thunk (use raw fn.toString()) → RED
    const messageFn = vi.fn(() => "computed message");
    await cons.withSpinner({ message: messageFn }, () => Promise.resolve(null));
    expect(messageFn).toHaveBeenCalled();
    const calls = host.log.info.mock.calls.map((c) => c[0] as string);
    expect(calls.some((msg) => msg.includes("computed message"))).toBe(true);
  });
});
