// Unit tests for the oneShot worker-close orchestration (worker-close.ts):
//
//   - a post-run close failure after a SUCCESSFUL run must be non-fatal;
//   - a pre-run close that hits a busy (abandoned) worker must recover via
//     a forceclose, not surface the bare "worker is busy" protocol error;
//   - after a FAILED run, the oneShot worker must still be closed
//     (best-effort: close, busy → forceclose, and no close failure may
//     mask the original run error).
//
// The mock boundary is the QNR command writer (writeJuliaCommand). No socket,
// no julia, no quarto internals — worker-close.ts is a pure module so this
// imports it standalone.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  errorRunClose,
  isWorkerBusyError,
  preRunClose,
  postRunClose,
  type CloseCommandWriter,
} from "../../src/worker-close.ts";

const FILE = "/tmp/sleepy.qmd";

// The verbatim shape QNR returns when a close hits a still-busy worker.
const BUSY_MESSAGE =
  `Julia server returned error after receiving "close" command:\n\n` +
  `Failed to close notebook: ${FILE}\n\n` +
  `The underlying Julia error was:\n\n` +
  `Tried to close file "${FILE}" but the corresponding worker is busy.`;

// Build a mock writer that records the command types it received and reacts
// per a caller-supplied table. Anything not in the table resolves to `true`
// (isopen-truthy / generic ack).
function mockWriter(
  reactions: Partial<Record<string, () => Promise<unknown>>>,
): { write: CloseCommandWriter; sent: string[] } {
  const sent: string[] = [];
  const write: CloseCommandWriter = (command) => {
    sent.push(command.type);
    const react = reactions[command.type];
    return react ? react() : Promise.resolve(true);
  };
  return { write, sent };
}

// ---------------------------------------------------------------------------
// Post-run close is non-fatal
// ---------------------------------------------------------------------------

test("a busy post-run close after a successful run is non-fatal (warns, resolves)", async () => {
  const warnings: string[] = [];
  const { write, sent } = mockWriter({
    close: () => Promise.reject(new Error(BUSY_MESSAGE)),
  });

  // Must NOT throw — the run already succeeded; cleanup failure != run failure.
  await postRunClose(write, FILE, (m) => warnings.push(m));

  assert.deepEqual(sent, ["close"]);
  assert.deepEqual(warnings.length, 1, "exactly one warning should be logged");
  assert.match(warnings[0], /worker is busy/i);
});

test("a clean post-run close does not warn", async () => {
  const warnings: string[] = [];
  const { write, sent } = mockWriter({
    close: () => Promise.resolve({ status: true }),
  });

  await postRunClose(write, FILE, (m) => warnings.push(m));

  assert.deepEqual(sent, ["close"]);
  assert.deepEqual(warnings.length, 0, "a successful close must not warn");
});

// ---------------------------------------------------------------------------
// Pre-run close-busy recovers via forceclose
// ---------------------------------------------------------------------------

test("a busy pre-run close recovers via forceclose and does not throw", async () => {
  const { write, sent } = mockWriter({
    isopen: () => Promise.resolve(true),
    close: () => Promise.reject(new Error(BUSY_MESSAGE)),
    forceclose: () => Promise.resolve({ status: true }),
  });

  // Must NOT throw — the abandoned worker is reclaimed by the forceclose.
  await preRunClose(write, FILE);

  assert.deepEqual(
    sent,
    ["isopen", "close", "forceclose"],
    "busy close must fall back to forceclose",
  );
});

test("a non-busy pre-run close error is NOT swallowed (propagates, no forceclose)", async () => {
  const { write, sent } = mockWriter({
    isopen: () => Promise.resolve(true),
    close: () => Promise.reject(new Error("some other server error")),
  });

  await assert.rejects(
    () => preRunClose(write, FILE),
    Error,
    "some other server error",
  );
  assert.deepEqual(
    sent,
    ["isopen", "close"],
    "a non-busy error must not trigger a forceclose",
  );
});

test("a pre-run forceclose that itself fails PROPAGATES (a failed forced close is a real environment failure)", async () => {
  const forcecloseError = "forceclose failed: control server unreachable";
  const { write, sent } = mockWriter({
    isopen: () => Promise.resolve(true),
    close: () => Promise.reject(new Error(BUSY_MESSAGE)),
    forceclose: () => Promise.reject(new Error(forcecloseError)),
  });

  // The busy fallback is the last line of defense; if the forced close ALSO
  // fails, that is a genuine environment failure and must surface (not be
  // swallowed, not be retried). The sequence stops at forceclose — no run.
  await assert.rejects(
    () => preRunClose(write, FILE),
    Error,
    forcecloseError,
  );
  assert.deepEqual(
    sent,
    ["isopen", "close", "forceclose"],
    "forceclose is attempted after the busy close; its own failure propagates unchanged",
  );
});

test("a closed (not-open) file skips the pre-run close entirely", async () => {
  const { write, sent } = mockWriter({
    isopen: () => Promise.resolve(false),
  });

  await preRunClose(write, FILE);

  assert.deepEqual(sent, ["isopen"], "no close should be sent when the file is not open");
});

// ---------------------------------------------------------------------------
// Error-path close (run failed; worker must not leak)
// ---------------------------------------------------------------------------

test("a clean close after a failed run closes the worker without warning", async () => {
  const warnings: string[] = [];
  const { write, sent } = mockWriter({
    close: () => Promise.resolve({ status: true }),
  });

  // The common case: QNR reported the cell error in-band, so the worker is
  // idle and healthy — a plain close suffices.
  await errorRunClose(write, FILE, (m) => warnings.push(m));

  assert.deepEqual(sent, ["close"]);
  assert.deepEqual(warnings.length, 0, "a successful close must not warn");
});

test("a busy close after a failed run escalates to forceclose", async () => {
  const warnings: string[] = [];
  const { write, sent } = mockWriter({
    close: () => Promise.reject(new Error(BUSY_MESSAGE)),
    forceclose: () => Promise.resolve({ status: true }),
  });

  // The transport-error case: the run threw without QNR finishing, so the
  // worker may still be busy. We are abandoning it either way — forceclose
  // prevents creating the abandoned-busy state preRunClose has to mop up.
  await errorRunClose(write, FILE, (m) => warnings.push(m));

  assert.deepEqual(
    sent,
    ["close", "forceclose"],
    "busy close must fall back to forceclose",
  );
  assert.deepEqual(warnings.length, 0, "a successful forceclose recovery must not warn");
});

test("a non-busy close failure after a failed run warns and resolves (never throws)", async () => {
  const warnings: string[] = [];
  const { write, sent } = mockWriter({
    close: () => Promise.reject(new Error("some other server error")),
  });

  // Must NOT throw — the caller is about to rethrow the ORIGINAL run error,
  // and a close failure must not mask it.
  await errorRunClose(write, FILE, (m) => warnings.push(m));

  assert.deepEqual(sent, ["close"], "a non-busy error must not trigger a forceclose");
  assert.deepEqual(warnings.length, 1, "exactly one warning should be logged");
  assert.match(warnings[0], /some other server error/);
});

test("an error-path forceclose that itself fails warns and resolves (unlike the pre-run close — the run error must surface)", async () => {
  const warnings: string[] = [];
  const { write, sent } = mockWriter({
    close: () => Promise.reject(new Error(BUSY_MESSAGE)),
    forceclose: () => Promise.reject(new Error("forceclose failed: control server unreachable")),
  });

  // Deliberate asymmetry with the pre-run forceclose-propagates contract: in
  // the error path there is already a real diagnostic in flight (the run
  // error); replacing it with a cleanup failure would mask the actual problem.
  await errorRunClose(write, FILE, (m) => warnings.push(m));

  assert.deepEqual(sent, ["close", "forceclose"]);
  assert.deepEqual(warnings.length, 1, "exactly one warning should be logged");
  assert.match(warnings[0], /control server unreachable/);
});

// ---------------------------------------------------------------------------
// Busy-error classification
// ---------------------------------------------------------------------------

test("isWorkerBusyError matches the QNR busy message and nothing else", () => {
  assert.ok(isWorkerBusyError(new Error(BUSY_MESSAGE)));
  assert.ok(!isWorkerBusyError(new Error("some other server error")));
  assert.ok(!isWorkerBusyError("worker is busy")); // non-Error input
  assert.ok(!isWorkerBusyError(undefined));
});
