/**
 * B2 — QuartoAPI conformance type-level tests (Plan 2 B2).
 *
 * These are compile-time assertions, not runtime tests: they are covered by
 * `( cd ts-packages/quarto-engine-host-deno && npx tsc --noEmit )`. The file is
 * NOT matched by vitest's `src/**\/*.test.ts` include (it is a `.type-test.ts`),
 * so it never runs — it only has to typecheck.
 *
 * Mechanism: a `.type-test.ts` cannot rely on a bare `@ts-expect-error` (which
 * passes on ANY error). Instead each assertion feeds a boolean into
 * `Expect<T extends true>`, so a violated relationship makes `tsc` ERROR on the
 * exact line. Each block documents the NAMED REVERT that flips it RED.
 */

import type { QuartoAPI } from "@quarto/types";
import type { SystemNamespace } from "@quarto/api/system";
import type { ConsoleNamespace } from "@quarto/api/console";
import type { PathHostNamespace } from "@quarto/api/path";
import type { MappedStringHostNamespace } from "@quarto/api/mappedString";

// ── type-level assertion helpers (no dep) ──────────────────────────────────────

/** Compiles only when `T` is exactly `true`. */
type Expect<T extends true> = T;

/** `true` iff `A` is assignable to `B`. */
type IsAssignable<A, B> = A extends B ? true : false;

/** `true` iff `A` and `B` are the *exact* same type (invariant identity trick). */
type IsExactly<A, B> =
  (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2)
    ? true
    : false;

// ═══════════════════════════════════════════════════════════════════════════
// T-B2-conform — the assembled namespaces conform to the SDK contract, and a
// mis-wired namespace is REJECTED (conformance is real, not `any`/loose).
// ═══════════════════════════════════════════════════════════════════════════

// (1) The production `@quarto/api` namespace types conform to `QuartoAPI[ns]`.
//     NAMED REVERT: loosen a derive (e.g. revert `SystemNamespace` back to the
//     pre-B2 loose stub where `runExternalPreviewServer(...args: unknown[]):
//     Promise<unknown>`) → `SystemNamespace` no longer conforms → RED here.
type _SystemConforms = Expect<IsAssignable<SystemNamespace, QuartoAPI["system"]>>;
type _ConsoleConforms = Expect<IsAssignable<ConsoleNamespace, QuartoAPI["console"]>>;
// The mostly-pure factories return only the host SUBSET (Pick), so they conform
// to the corresponding slice of the SDK namespace, not the whole thing.
type _PathHostConforms = Expect<
  IsAssignable<PathHostNamespace, Pick<QuartoAPI["path"], "absolute" | "runtime" | "resource" | "dataDir">>
>;
type _MappedStringHostConforms = Expect<
  IsAssignable<MappedStringHostNamespace, Pick<QuartoAPI["mappedString"], "fromFile">>
>;

// (2) A deliberately mis-wired system namespace — `runExternalPreviewServer`
//     regressed to the pre-B2 loose async stub — must NOT be assignable to the
//     production `SystemNamespace`. This is the negative test: the position is
//     strict enough to catch a real mis-wire.
//     NAMED REVERT: revert the `SystemNamespace` derive (its
//     `runExternalPreviewServer` back to `(...args: unknown[]) => Promise<unknown>`)
//     → `MisWiredSystem` becomes assignable → `extends false` becomes false →
//     `Expect<false>` → RED.
type MisWiredSystem = Omit<SystemNamespace, "runExternalPreviewServer"> & {
  runExternalPreviewServer: (...args: unknown[]) => Promise<unknown>;
};
type _MisWireRejected = Expect<
  IsAssignable<MisWiredSystem, SystemNamespace> extends false ? true : false
>;

// ═══════════════════════════════════════════════════════════════════════════
// T-B2-onCleanup — the vendored handler param is EXACTLY `() => void`.
//   NAMED REVERT: re-widen `@quarto/types` `onCleanup` to
//   `(handler: () => void | Promise<void>) => void` → the param type widens →
//   `IsExactly<…, () => void>` becomes false → `Expect<false>` → RED.
//   (We use exact identity, NOT `expectError` on an async handler: void-return
//   assignability lets `() => Promise<void>` satisfy `() => void`, so an
//   `expectError` there would be unmountable.)
// ═══════════════════════════════════════════════════════════════════════════
type _OnCleanupParamExact = Expect<
  IsExactly<Parameters<QuartoAPI["system"]["onCleanup"]>[0], () => void>
>;

// Reference the aliases so `noUnusedLocals`-style checks (if enabled) stay quiet
// and the assertions are unambiguously "used".
export type __B2TypeTests = [
  _SystemConforms,
  _ConsoleConforms,
  _PathHostConforms,
  _MappedStringHostConforms,
  _MisWireRejected,
  _OnCleanupParamExact,
];
