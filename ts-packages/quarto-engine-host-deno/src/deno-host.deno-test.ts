/**
 * Deno-native tests for denoHost.fs.walk.
 *
 * Run from the repo root:
 *   deno test --allow-all ts-packages/quarto-engine-host-deno/src/deno-host.deno-test.ts
 *
 * This file is excluded from the tsc/vitest graph:
 * - tsconfig.json: exclude "src/**\/*.deno-test.ts"
 * - vitest.config.ts: exclude "**\/*.deno-test.ts"
 */
import { assertEquals } from "jsr:@std/assert";
import { isAbsolute } from "jsr:@std/path";
import { denoHost } from "./deno-host.ts";

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
