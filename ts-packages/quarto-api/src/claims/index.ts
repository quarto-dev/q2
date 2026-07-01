/**
 * Claim constructors for {@link ExecutionEngineDiscovery.claimsLanguage}.
 *
 * These helpers produce the author-SDK {@link LanguageClaim} shape so engine
 * authors write `claimsLanguage: (lang) => lang === "julia" ? primary() : null`
 * instead of spelling out `{ kind: "primary" }` by hand.
 *
 * **Priority is intentionally omitted from no-arg calls** — the harness
 * (`@quarto/engine-host-deno`) owns the defaults (primary→1, interop/fallback→0)
 * and fills them in when normalizing to the wire type. Baking defaults here
 * would duplicate that logic and risk drift.
 */

import type { LanguageClaim } from "@quarto/types";

/**
 * Claim this language as a primary engine.
 *
 * @param priority - Optional priority. When omitted, the harness defaults to 1.
 *   Higher numbers win over lower numbers within the `primary` kind; use a
 *   custom value to out-bid or under-bid the built-in engines.
 */
export function primary(priority?: number): LanguageClaim {
  return priority === undefined ? { kind: "primary" } : { kind: "primary", priority };
}

/**
 * Claim this language as an interop engine (presence-gated: only extends
 * ownership when this engine is already in the sequence via a `primary` claim).
 *
 * @param priority - Optional priority. When omitted, the harness defaults to 0.
 */
export function interop(priority?: number): LanguageClaim {
  return priority === undefined ? { kind: "interop" } : { kind: "interop", priority };
}

/**
 * Claim this language as a fallback engine (universal kernel; lowest precedence).
 *
 * @param priority - Optional priority. When omitted, the harness defaults to 0.
 */
export function fallback(priority?: number): LanguageClaim {
  return priority === undefined ? { kind: "fallback" } : { kind: "fallback", priority };
}
