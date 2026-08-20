/**
 * T-Gate-parity — TS↔Rust wire-dual parity (Plan 2 Phase B gate).
 *
 * Run from the repo root:
 *   deno test --allow-all ts-packages/quarto-engine-host-deno/src/wire-parity.deno-test.ts
 *
 * WHY THIS EXISTS (tsc's blind spot):
 *   A green `tsc` proves TS-internal coherence only. It cannot see a Rust
 *   rename of a dual-defined wire field, and TS interface keys are ERASED at
 *   runtime. So we mechanize a two-sided parity check for the four wire-dual
 *   types shared by:
 *     - Rust: crates/quarto-core/src/engine/ts_protocol.rs
 *     - TS  : ./types.ts (this package)
 *
 * THE TWO SIDES:
 *   - Rust half: a regen-gated #[test] in ts_protocol.rs serializes one
 *     canonical instance of each wire-dual type (ALL optionals populated) to the
 *     committed fixture below. The fixture is a real serde ARTIFACT — so a Rust
 *     wire-key rename actually changes the fixture's key-set.
 *   - TS half (here): for each type we set-equate `Object.keys(fixtureInstance)`
 *     against a `KEYS` list pinned to the TS wire type via
 *     `satisfies readonly (keyof T)[]` + an `_Exhaustive` compile guard. The
 *     set-equality is SYMMETRIC: a Rust-only key (in fixture, not in KEYS)
 *     fails, AND a TS-only key (in KEYS/type, not in fixture) fails.
 *
 * Together the three are bound: Rust shape (fixture), TS shape (type, via the
 * `satisfies`/`_Exhaustive` compile guards), and the runtime KEYS list.
 *
 * REGEN: if this test fails after a deliberate wire change, regenerate the Rust
 *   fixture with:
 *     QUARTO_REGEN_WIRE_FIXTURES=1 cargo nextest run -p quarto-core -E \
 *       'test(ts_protocol::tests::test_ts_wire_parity_fixture)'
 *   and update the matching TS type + KEYS list in ./types.ts.
 *
 * This file is excluded from the tsc/vitest graph:
 * - tsconfig.json: exclude "src/**\/*.deno-test.ts"
 * - vitest.config.ts: exclude "**\/*.deno-test.ts"
 * The `satisfies`/`_Exhaustive` compile guards are instead checked by
 * `deno test`'s own type-checker (and are documented for the CI step).
 */
import { assert, assertEquals } from "jsr:@std/assert";
import type {
  EngineProjectContext,
  HostGlobalConfig,
  TsLanguageClaim,
  TsPandocIncludes,
} from "./types.ts";

// ─── fixture ─────────────────────────────────────────────────────────────────

// Resolve the committed Rust-serialized fixture relative to THIS file (cwd
// independent). From src/ up to the repo root is three levels
// (src → quarto-engine-host-deno → ts-packages → repo root), hence three `../`.
const FIXTURE_URL = new URL(
  "../../../crates/quarto-core/tests/fixtures/ts_wire_parity.json",
  import.meta.url,
);

type Fixture = Record<string, Record<string, unknown>>;
const fixture: Fixture = JSON.parse(await Deno.readTextFile(FIXTURE_URL));

// ─── type-pinned KEYS lists ──────────────────────────────────────────────────
//
// Each list is `satisfies readonly (keyof T)[]` — a KEYS entry that is NOT a
// field of the TS type fails to COMPILE. The paired `_Exhaustive*` guard is the
// other direction: it errors if the TS type has a field MISSING from KEYS.
// Together they pin KEYS ≡ keyof T at compile time.

const HOST_GLOBAL_CONFIG_KEYS = [
  "resourceDir",
  "runtimeDir",
  "dataDir",
  "pandocPath",
  "isInteractiveSession",
  "runningInCi",
  "quartoVersion",
] as const satisfies readonly (keyof HostGlobalConfig)[];
type _ExhaustiveHostGlobalConfig =
  Exclude<keyof HostGlobalConfig, (typeof HOST_GLOBAL_CONFIG_KEYS)[number]> extends never
    ? true
    : never;

const TS_PANDOC_INCLUDES_KEYS = [
  "inHeader",
  "beforeBody",
  "afterBody",
] as const satisfies readonly (keyof TsPandocIncludes)[];
type _ExhaustiveTsPandocIncludes =
  Exclude<keyof TsPandocIncludes, (typeof TS_PANDOC_INCLUDES_KEYS)[number]> extends never
    ? true
    : never;

const ENGINE_PROJECT_CONTEXT_KEYS = [
  "projectDir",
  "isSingleFile",
  "config",
  "outputDir",
] as const satisfies readonly (keyof EngineProjectContext)[];
type _ExhaustiveEngineProjectContext =
  Exclude<keyof EngineProjectContext, (typeof ENGINE_PROJECT_CONTEXT_KEYS)[number]> extends never
    ? true
    : never;

// `TsLanguageClaim` is a discriminated union; `keyof` a union is the
// INTERSECTION of member keys = "kind" | "priority" (both members share them).
const TS_LANGUAGE_CLAIM_KEYS = [
  "kind",
  "priority",
] as const satisfies readonly (keyof TsLanguageClaim)[];
type _ExhaustiveTsLanguageClaim =
  Exclude<keyof TsLanguageClaim, (typeof TS_LANGUAGE_CLAIM_KEYS)[number]> extends never
    ? true
    : never;

// Reference the guard aliases so an unused-type lint (if any) can't strip them;
// each is `true` when KEYS is exhaustive, `never` (a compile error at the
// assignment) otherwise.
const _guards: [
  _ExhaustiveHostGlobalConfig,
  _ExhaustiveTsPandocIncludes,
  _ExhaustiveEngineProjectContext,
  _ExhaustiveTsLanguageClaim,
] = [true, true, true, true];
void _guards;

// ─── symmetric set-equality ──────────────────────────────────────────────────

/**
 * Assert two string collections are set-equal (order-independent, both
 * directions). Reports the missing/extra keys by name on failure.
 */
function assertSetEqual(
  actual: readonly string[],
  expected: readonly string[],
  label: string,
): void {
  const actualSet = new Set(actual);
  const expectedSet = new Set(expected);
  const missingFromActual = [...expectedSet].filter((k) => !actualSet.has(k)).sort();
  const extraInActual = [...actualSet].filter((k) => !expectedSet.has(k)).sort();
  assertEquals(
    { missingFromActual, extraInActual },
    { missingFromActual: [], extraInActual: [] },
    `${label}: wire key-set mismatch. ` +
      `Rust-fixture-only keys (missing from TS KEYS): [${extraInActual.join(", ")}]; ` +
      `TS-KEYS-only keys (missing from Rust fixture): [${missingFromActual.join(", ")}].`,
  );
}

/** Fetch a fixture instance, failing loudly if the Rust side didn't emit it. */
function instance(typeName: string): Record<string, unknown> {
  const v = fixture[typeName];
  assert(
    v !== undefined && typeof v === "object",
    `fixture is missing an instance for '${typeName}' — regenerate the Rust ` +
      `fixture (QUARTO_REGEN_WIRE_FIXTURES=1 …).`,
  );
  return v;
}

// ─── tests ───────────────────────────────────────────────────────────────────

Deno.test("wire-parity: HostGlobalConfig keys match Rust fixture", () => {
  assertSetEqual(
    Object.keys(instance("HostGlobalConfig")),
    HOST_GLOBAL_CONFIG_KEYS,
    "HostGlobalConfig",
  );
});

Deno.test("wire-parity: TsPandocIncludes keys match Rust fixture", () => {
  assertSetEqual(
    Object.keys(instance("TsPandocIncludes")),
    TS_PANDOC_INCLUDES_KEYS,
    "TsPandocIncludes",
  );
});

Deno.test("wire-parity: EngineProjectContext keys match Rust fixture", () => {
  assertSetEqual(
    Object.keys(instance("EngineProjectContext")),
    ENGINE_PROJECT_CONTEXT_KEYS,
    "EngineProjectContext",
  );
});

Deno.test("wire-parity: TsLanguageClaim keys match Rust fixture", () => {
  assertSetEqual(
    Object.keys(instance("TsLanguageClaim")),
    TS_LANGUAGE_CLAIM_KEYS,
    "TsLanguageClaim",
  );
});
