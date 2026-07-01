/**
 * Deno-native tests for denoHost (fs.walk and process.exec knobs).
 *
 * Run from the repo root:
 *   deno test --allow-all ts-packages/quarto-engine-host-deno/src/deno-host.deno-test.ts
 *
 * This file is excluded from the tsc/vitest graph:
 * - tsconfig.json: exclude "src/**\/*.deno-test.ts"
 * - vitest.config.ts: exclude "**\/*.deno-test.ts"
 */
import { assertEquals, assertRejects } from "jsr:@std/assert";
import { isAbsolute } from "jsr:@std/path";
import { denoHost } from "./deno-host.ts";
// ExecOptions is imported as a type (platform/index.ts has no .js re-exports,
// so Deno's type-checker resolves it cleanly without --sloppy-imports).
import type { ExecOptions } from "@quarto/api/platform";

// ─── helpers ───────────────────────────────────────────────────────────────

/** Create the standard test tree under a fresh temp dir and return its root. */
function makeTempTree(): string {
  const root = denoHost.fs.makeTempDir({ prefix: "walk-test-" });
  Deno.writeTextFileSync(`${root}/a.txt`, "a");
  Deno.mkdirSync(`${root}/sub`);
  Deno.writeTextFileSync(`${root}/sub/b.txt`, "b");
  Deno.mkdirSync(`${root}/sub/deep`);
  return root;
}

// ─── tests ─────────────────────────────────────────────────────────────────

/**
 * Default walk: files only, no depth limit.
 *
 * Named-revert binding:
 *   REVERT "drop includeDirs default" → includeDirs becomes true →
 *   directories (sub, sub/deep, root) appear in results → length > 2 → FAIL
 */
Deno.test("denoHost.fs.walk - default returns exactly the two files", () => {
  const root = makeTempTree();
  try {
    const entries = denoHost.fs.walk(root);

    // Should contain exactly the two files
    assertEquals(entries.length, 2, `Expected 2 entries, got ${entries.length}: ${JSON.stringify(entries.map(e => e.path))}`);

    const paths = entries.map((e) => e.path);

    // All paths must be absolute
    for (const p of paths) {
      assertEquals(isAbsolute(p), true, `Expected absolute path, got: ${p}`);
    }

    // Both files present
    assertEquals(
      paths.some((p) => p.endsWith("/a.txt")),
      true,
      "Expected a.txt in results",
    );
    assertEquals(
      paths.some((p) => p.endsWith("/sub/b.txt")),
      true,
      "Expected sub/b.txt in results",
    );

    // No directories or root
    assertEquals(
      paths.some((p) => p === root),
      false,
      "Root dir must not appear in default walk",
    );
    assertEquals(
      paths.some((p) => p.endsWith("/sub") && !p.endsWith("/sub/b.txt")),
      false,
      "'sub' dir must not appear in default walk",
    );
    assertEquals(
      paths.some((p) => p.endsWith("/deep")),
      false,
      "'sub/deep' dir must not appear in default walk",
    );

    /**
     * Named-revert binding:
     *   REVERT "hard-code isFile:true/isDirectory:false" in walk map →
     *   isFile is always true, isDirectory always false →
     *   the assertions below still pass (they're for files) —
     *   BUT the includeDirs:true test below would catch the revert.
     *   See that test for the failing assertion.
     */
    for (const e of entries) {
      assertEquals(e.isFile, true, `Expected isFile:true for ${e.path}`);
      assertEquals(
        e.isDirectory,
        false,
        `Expected isDirectory:false for ${e.path}`,
      );
    }
  } finally {
    denoHost.fs.remove(root, { recursive: true });
  }
});

/**
 * maxDepth: 1 — only direct children of root, no subdirectory files.
 *
 * Named-revert binding:
 *   REVERT "drop maxDepth pass-through" → walkSync gets no maxDepth →
 *   defaults to Infinity → sub/b.txt appears → length > 1 → FAIL
 */
Deno.test("denoHost.fs.walk - maxDepth:1 includes a.txt but NOT sub/b.txt", () => {
  const root = makeTempTree();
  try {
    const entries = denoHost.fs.walk(root, { maxDepth: 1 });
    const paths = entries.map((e) => e.path);

    assertEquals(
      entries.length,
      1,
      `Expected 1 entry (a.txt only), got ${entries.length}: ${JSON.stringify(paths)}`,
    );
    assertEquals(
      paths.some((p) => p.endsWith("/a.txt")),
      true,
      "Expected a.txt in maxDepth:1 results",
    );
    assertEquals(
      paths.some((p) => p.endsWith("/sub/b.txt")),
      false,
      "sub/b.txt must NOT appear under maxDepth:1",
    );
  } finally {
    denoHost.fs.remove(root, { recursive: true });
  }
});

/**
 * includeDirs: true — directories included with correct flags.
 *
 * Named-revert binding:
 *   REVERT "hard-code isFile:true/isDirectory:false" in walk map →
 *   dir entries have isDirectory:false → assertEquals(e.isDirectory, true) → FAIL
 *
 * Also catches: REVERT "drop includeDirs flag" → with true default, the
 * library already includes dirs, so this test would still pass — the
 * binding is on the hard-code revert.
 */
Deno.test("denoHost.fs.walk - includeDirs:true includes sub and sub/deep with isDirectory:true", () => {
  const root = makeTempTree();
  try {
    const entries = denoHost.fs.walk(root, { includeDirs: true });
    const dirEntries = entries.filter((e) => e.isDirectory);
    const dirPaths = dirEntries.map((e) => e.path);

    // sub/ and sub/deep/ must be present
    assertEquals(
      dirPaths.some((p) => p.endsWith("/sub") && !p.endsWith("/sub/deep")),
      true,
      "'sub' dir must appear when includeDirs:true",
    );
    assertEquals(
      dirPaths.some((p) => p.endsWith("/sub/deep")),
      true,
      "'sub/deep' dir must appear when includeDirs:true",
    );

    // All dir entries must have the correct flags
    for (const e of dirEntries) {
      assertEquals(
        e.isFile,
        false,
        `Dir entry ${e.path} must have isFile:false`,
      );
      assertEquals(
        e.isDirectory,
        true,
        `Dir entry ${e.path} must have isDirectory:true`,
      );
    }
  } finally {
    denoHost.fs.remove(root, { recursive: true });
  }
});

// ─── T-B1b tests: execProcess knobs via denoHost.process.exec (Plan 2 B1) ──
//
// These tests call denoHost.process.exec directly with ExecOptions to verify
// the four knobs have real bodies in denoHost.  Using exec directly (rather
// than makeSystem) avoids a Deno type-check limitation: makeSystem's source
// imports platform/index.js with a .js extension which Deno can't resolve
// without --sloppy-imports.  The marshalling path (positional args → ExecOptions)
// is already proven at the vitest tier (T-B1a, T-B1c-gate).

const denoExe = Deno.execPath();

/**
 * T-B1b-merge: stdout>stderr merge delivers stdout content into ExecResult.stderr.
 *
 * Vacuity guard: child MUST write to BOTH stdout and stderr; the assertion
 * targets the STDOUT content ("STDOUT_CONTENT") appearing in the merged stderr
 * sink — writing to stdout only would pass with or without merge.
 *
 * T-B1c-gate (deno tier, same test): ExecResult.stdout must be "" (empty)
 * because all output was merged into stderr.
 *
 * Named-revert binding (T-B1b-merge + T-B1c-gate):
 *   REVERT "mergeOutput routing in denoHost" → streams processed independently →
 *   "STDOUT_CONTENT" stays in result.stdout, NOT in result.stderr → FAIL
 *   AND result.stdout is non-empty → T-B1c-gate assertion fails → FAIL
 */
Deno.test("T-B1b-merge + T-B1c-gate: stdout>stderr merge — stdout content in result.stderr AND result.stdout='' (merge-routing binding)", async () => {
  // Child writes distinguishable text to BOTH stdout AND stderr.
  const script =
    `Deno.stdout.writeSync(new TextEncoder().encode("STDOUT_CONTENT")); ` +
    `Deno.stderr.writeSync(new TextEncoder().encode("STDERR_CONTENT"));`;

  const opts: ExecOptions = { mergeOutput: "stdout>stderr" };
  const result = await denoHost.process.exec(denoExe, ["eval", script], opts);

  // T-B1b-merge: stdout content ("STDOUT_CONTENT") must appear in the merged stderr sink.
  // Revert → RED: without merge routing, stdout content stays in result.stdout, not result.stderr.
  assertEquals(
    result.stderr.includes("STDOUT_CONTENT"),
    true,
    `Expected "STDOUT_CONTENT" in result.stderr, got: ${JSON.stringify(result.stderr)}`,
  );

  // T-B1c-gate: result.stdout must be empty because all output was merged into stderr.
  // Revert → RED: without merge routing, result.stdout contains "STDOUT_CONTENT" instead of "".
  assertEquals(
    result.stdout,
    "",
    `Expected result.stdout="" (merged to stderr), got: ${JSON.stringify(result.stdout)}`,
  );
});

/**
 * T-B1b-filter: stderrFilter is applied per-chunk to stderr output.
 *
 * Named-revert binding:
 *   REVERT "stderrFilter application in denoHost" → raw stderr returned →
 *   result.stderr contains "ORIGINAL_ERR" without prefix → startsWith("FILTERED:") fails → FAIL
 */
Deno.test("T-B1b-filter: stderrFilter transforms each stderr chunk (filter-application binding)", async () => {
  // Child writes known text to stderr.
  const script = `Deno.stderr.writeSync(new TextEncoder().encode("ORIGINAL_ERR"));`;

  const opts: ExecOptions = {
    stderrFilter: (s: string) => `FILTERED:${s}`,
  };
  const result = await denoHost.process.exec(denoExe, ["eval", script], opts);

  // Revert → RED: without filter, result.stderr is "ORIGINAL_ERR" (no "FILTERED:" prefix) → FAIL
  assertEquals(
    result.stderr.startsWith("FILTERED:"),
    true,
    `Expected result.stderr to start with "FILTERED:", got: ${JSON.stringify(result.stderr)}`,
  );
});

/**
 * T-B1b-merge+filter: mergeOutput AND stderrFilter applied together.
 *
 * Vacuity guard: child writes distinguishable content to BOTH stdout AND stderr.
 * The assertion targets the merged stderr sink (per "stdout>stderr") containing:
 *   (a) the stdout content as-is, AND
 *   (b) the stderr content WITH the "F:" filter prefix applied.
 * Reverting either the merge routing OR the stderrFilter application breaks it.
 *
 * Named-revert binding:
 *   REVERT merge routing → stdout content stays in result.stdout, not stderr → FAIL
 *   REVERT stderrFilter application → stderr content lacks "F:" prefix → FAIL
 */
Deno.test("T-B1b-merge+filter: mergeOutput+stderrFilter together — merged sink has stdout content AND filtered stderr", async () => {
  // Child writes distinguishable text to BOTH stdout AND stderr.
  const script =
    `Deno.stdout.writeSync(new TextEncoder().encode("STDOUT_PART")); ` +
    `Deno.stderr.writeSync(new TextEncoder().encode("STDERR_PART"));`;

  const opts: ExecOptions = {
    mergeOutput: "stdout>stderr",
    stderrFilter: (s: string) => `F:${s}`,
  };
  const result = await denoHost.process.exec(denoExe, ["eval", script], opts);

  // Merged sink (stderr) must contain the raw stdout content.
  // Revert merge routing → "STDOUT_PART" only in result.stdout → this fails.
  assertEquals(
    result.stderr.includes("STDOUT_PART"),
    true,
    `Expected "STDOUT_PART" in merged result.stderr, got: ${JSON.stringify(result.stderr)}`,
  );

  // Merged sink must also contain the filtered ("F:"-prefixed) stderr content.
  // Revert stderrFilter application → "STDERR_PART" present without prefix → this fails.
  assertEquals(
    result.stderr.includes("F:STDERR_PART"),
    true,
    `Expected "F:STDERR_PART" in result.stderr (filter applied), got: ${JSON.stringify(result.stderr)}`,
  );
});

/**
 * T-B1b-timeout: process killed when timeout elapses.
 *
 * Vacuity guard: child sleeps 10 seconds; timeout is 300ms — child MUST
 * outlast the timeout so the race is meaningful.
 *
 * Named-revert binding:
 *   REVERT "Promise.race / child.kill() in denoHost" → sleeper completes
 *   (after ~10 s) without rejection → assertRejects sees no rejection → FAIL
 */
Deno.test("T-B1b-timeout: process killed when timeout elapses (timeout-kill binding)", async () => {
  // Child sleeps 10 seconds — guaranteed to outlast the 300ms timeout.
  const script = `await new Promise(r => setTimeout(r, 10_000));`;

  const opts: ExecOptions = { timeout: 300 }; // 300 ms << 10 000 ms child sleep

  // Revert → RED: without timeout/kill, the child completes after 10 s and
  //   assertRejects sees no rejection → test fails.
  await assertRejects(
    () => denoHost.process.exec(denoExe, ["eval", script], opts),
    Error,
    "timed out",
  );
});
