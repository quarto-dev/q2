/**
 * bd-x3h4zjhl — `ExecuteOptions.executeEnv` type-level tests.
 *
 * Compile-time only (checked by `npm run typecheck -w @quarto/engine-host-node`;
 * not matched by vitest). Same mechanism as `b2-conformance.type-test.ts`.
 *
 * Named revert: delete `executeEnv` from `ExecuteOptions` in
 * `ts-packages/quarto-types/src/execution.ts` -> assertion (1) errors.
 */

import type { ExecuteOptions } from "@quarto/types";

/** Compiles only when `T` is exactly `true`. */
type Expect<T extends true> = T;

/** `true` iff `A` is assignable to `B`. */
type IsAssignable<A, B> = [A] extends [B] ? true : false;

// (1) A readonly string record is assignable to the field.
export type _ReadonlyRecordAssignable = Expect<
  IsAssignable<Readonly<Record<string, string>>, ExecuteOptions["executeEnv"]>
>;

// (2) The field is optional: an options object without it still type-checks.
type WithoutField = Omit<ExecuteOptions, "executeEnv">;
export type _FieldIsOptional = Expect<IsAssignable<WithoutField, ExecuteOptions>>;
