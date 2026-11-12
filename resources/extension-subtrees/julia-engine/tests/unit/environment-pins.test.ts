import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, test } from "node:test";

// `ensure_environment.jl` must re-resolve when the `[sources]` revision in Project.toml no
// longer matches the revision the cached Manifest.toml resolved: Pkg's own manifest check
// (`Pkg.is_manifest_current`) ignores `[sources]`, so a pin bump alone would keep the old
// QuartoNotebookRunner on every machine that already has a cached environment.

const helper = join(
  import.meta.dirname,
  "..",
  "..",
  "_extensions",
  "julia-engine",
  "environment_pins.jl",
);

const REV_A = "5e042849af546fdbe13dfac9157fc38c7acbe876";
const REV_B = "ffe5dab72066ddd91bad480fc30f861749e305d4";

const juliaPresent =
  spawnSync("julia", ["--version"], { encoding: "utf8" }).status === 0;

function project(rev: string | null): string {
  const sources = rev === null
    ? ""
    : `\n[sources]\nQuartoNotebookRunner = {url = "https://example.invalid/QNR.jl", rev = "${rev}"}\n`;
  return `[deps]\nQuartoNotebookRunner = "4c0109c6-14e9-4c88-93f0-2b974d3468f4"\n${sources}\n[compat]\nQuartoNotebookRunner = "=0.18.6"\n`;
}

function manifest(rev: string | null): string {
  const pin = rev === null ? "" : `repo-rev = "${rev}"\nrepo-url = "https://example.invalid/QNR.jl"\n`;
  return `julia_version = "1.12.0"\nmanifest_format = "2.0"\n\n[[deps.QuartoNotebookRunner]]\ngit-tree-sha1 = "abc"\n${pin}uuid = "4c0109c6-14e9-4c88-93f0-2b974d3468f4"\nversion = "0.18.6"\n`;
}

function pinsMatch(projectRev: string | null, manifestRev: string | null): boolean {
  const dir = mkdtempSync(join(tmpdir(), "pins-"));
  try {
    writeFileSync(join(dir, "Project.toml"), project(projectRev));
    writeFileSync(join(dir, "Manifest.toml"), manifest(manifestRev));
    const r = spawnSync(
      "julia",
      [
        "--startup-file=no",
        "-e",
        `include(ARGS[1]); exit(sources_pins_match(joinpath(ARGS[2], "Project.toml"), joinpath(ARGS[2], "Manifest.toml")) ? 0 : 3)`,
        helper,
        dir,
      ],
      { encoding: "utf8" },
    );
    // Exit 0 means match, 3 means mismatch; anything else (1 is Julia's error exit) is a failure.
    assert.ok(r.status === 0 || r.status === 3, `julia failed (${r.status}): ${r.stderr}`);
    return r.status === 0;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe("sources_pins_match", { skip: !juliaPresent && "julia is not installed" }, () => {
  test("the same revision in Project.toml and Manifest.toml matches", () => {
    assert.equal(pinsMatch(REV_A, REV_A), true);
  });

  test("a bumped [sources] revision does not match the cached manifest", () => {
    assert.equal(pinsMatch(REV_B, REV_A), false);
  });

  test("a manifest with no recorded revision does not match a pinned project", () => {
    assert.equal(pinsMatch(REV_A, null), false);
  });

  test("a project with no [sources] pin always matches (nothing to compare)", () => {
    assert.equal(pinsMatch(null, REV_A), true);
    assert.equal(pinsMatch(null, null), true);
  });
});
