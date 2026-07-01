/*
 * worker-close.ts
 *
 * oneShot worker-close orchestration for the Julia engine, extracted so the
 * close/busy recovery logic is unit-testable with a mocked command writer.
 */

// A narrow view of the QNR command writer (writeJuliaCommand). worker-close
// only ever emits isopen / close / forceclose; it treats the response as an
// opaque value (isopen is checked for truthiness, the closes for rejection).
export type CloseCommandWriter = (
  command: {
    type: "isopen" | "close" | "forceclose";
    content: { file: string };
  },
) => Promise<unknown>;

// A close fails with "worker is busy" when the file's worker is still running
// (QNR does not interrupt a running task on a plain close). See task-p0-report.md.
export function isWorkerBusyError(e: unknown): boolean {
  return e instanceof Error && /worker is busy/i.test(e.message);
}

// Pre-run close (julia-engine.ts oneShot / daemon-restart path). If the file's
// worker is busy we recover with a forceclose rather than surfacing the bare
// protocol error: the busy worker is an ABANDONED one (a prior client vanished
// mid-run and left the shared server's worker orphaned — the Bug A scenario),
// and forceclose reclaims the file so this fresh render can proceed.
//
// CAVEAT (documented for the upstream PR, deliberately not special-cased here):
// a worker busy serving a *live concurrent* render on a shared server would also
// be force-closed, killing legitimate work. Distinguishing abandoned-vs-live is
// the deeper oneShot-server-reuse design question — see the q2 compat log.
export async function preRunClose(
  writeCommand: CloseCommandWriter,
  file: string,
): Promise<void> {
  const isopen = await writeCommand({ type: "isopen", content: { file } });
  if (isopen) {
    try {
      await writeCommand({ type: "close", content: { file } });
    } catch (e) {
      if (isWorkerBusyError(e)) {
        // Last line of defense. If the forced close ITSELF fails, that is a
        // genuine environment failure (control server unreachable, etc.) — let
        // it propagate; do not swallow or retry.
        await writeCommand({ type: "forceclose", content: { file } });
      } else {
        throw e;
      }
    }
  }
}

// Post-run close (julia-engine.ts oneShot cleanup). The run has already
// SUCCEEDED at this point, so a failed cleanup close must not discard the
// results — warn and return. A cleanup failure is not an execution failure.
export async function postRunClose(
  writeCommand: CloseCommandWriter,
  file: string,
  warn: (message: string) => void,
): Promise<void> {
  try {
    await writeCommand({ type: "close", content: { file } });
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    warn(
      `Julia worker close after a successful run failed; returning results anyway.\n${message}`,
    );
  }
}
