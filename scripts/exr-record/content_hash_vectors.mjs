// Positron `quarto_content_hash` vectors (EXR-4 Phase 0).
//
// The rule (positron quartoDocumentModel.ts:22, quartoParser.ts:113,173; hash.ts:190-233):
// split the block content on /\r?\n/, rejoin with "\n", SHA-1 over UTF-8
// (a lone surrogate is encoded as U+FFFD), hex digest truncated to 16 chars.
//
//   node scripts/exr-record/content_hash_vectors.mjs           verify the fixture
//   node scripts/exr-record/content_hash_vectors.mjs --write   regenerate it
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const FIXTURE = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../crates/quarto-core/tests/fixtures/records/content-hash-vectors.json",
);

export function contentHash(blockContent) {
  const normalized = blockContent.split(/\r?\n/).join("\n");
  return createHash("sha1").update(Buffer.from(normalized, "utf8")).digest("hex").slice(0, 16);
}

// `js` is the input as Positron would hold it (it may contain a lone surrogate).
// `expected` is pinned where the plan gives it, or where Positron itself produced it (S1); null means "computed here, then pinned in the fixture".
const VECTORS = [
  { name: "simple", js: "x <- 1", expected: "4390f67858f3f1c7" },
  { name: "accent", js: "café", expected: "f424452a9673918c" },
  { name: "astral-emoji", js: "😀", expected: "9c533688a979a858" },
  { name: "label-line-and-unicode", js: "#| label: fig-é\nx <- 'é😀'", expected: "774a7e618b133ed6" },
  { name: "empty-cell", js: "", expected: null },
  { name: "lone-surrogate-is-replacement-char", js: "\ud800", expected: null },
  // Pinned to the value Positron's own StringSHA1 produced in EXR-16 spike S1 (2026-10-31,
  // claude-notes/research/2026-10-31-exr-16-s1-conformance-findings.md): a lone surrogate
  // hashes as U+FFFD, so this equals the hash of "a\ufffdb".
  { name: "lone-surrogate-mid-text-positron", js: "a\ud800b", expected: "c3693aea616c886c" },
  { name: "crlf-equals-lf", js: "#| label: a\r\nx <- 1\r\ny <- 2", expected: null,
    twin: "#| label: a\nx <- 1\ny <- 2" },
  { name: "trailing-newline-kept", js: "x <- 1\n", expected: null },
];

function build() {
  return VECTORS.map((v) => {
    const got = contentHash(v.js);
    if (v.expected !== null && got !== v.expected) {
      throw new Error(`${v.name}: computed ${got}, plan says ${v.expected}`);
    }
    if (v.twin !== undefined && contentHash(v.twin) !== got) {
      throw new Error(`${v.name}: CRLF input and its LF twin must hash alike`);
    }
    // Rust strings cannot hold a lone surrogate: store what Rust receives.
    const input = v.js.toWellFormed();
    if (contentHash(input) !== got) {
      throw new Error(`${v.name}: U+FFFD substitution changed the hash`);
    }
    return { name: v.name, input, expected: got };
  });
}

const vectors = build();
if (process.argv.includes("--write")) {
  writeFileSync(FIXTURE, JSON.stringify(vectors, null, 1) + "\n");
  console.log(`wrote ${vectors.length} vectors to ${FIXTURE}`);
} else {
  const stored = JSON.parse(readFileSync(FIXTURE, "utf8"));
  if (JSON.stringify(stored) !== JSON.stringify(vectors)) {
    console.error("fixture differs from recomputed vectors; run with --write to inspect");
    process.exit(1);
  }
  console.log(`ok: ${vectors.length} vectors match`);
}
