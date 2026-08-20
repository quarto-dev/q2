/**
 * T-B2-claims — claim constructor tests (Plan 2, Phase B).
 *
 * TDD: tests written before the implementation. Named-revert markers document
 * which assertion goes RED when the corresponding revert is applied.
 */

import { describe, it, expect } from "vitest";
import { primary, interop, fallback } from "./index.js";

describe("T-B2-claims — claim constructors", () => {
  it("primary() returns bare {kind:'primary'} when priority omitted", () => {
    // Named revert → RED: if primary() bakes { priority: 1 }, this fails (extra key).
    expect(primary()).toEqual({ kind: "primary" });
  });

  it("primary(2) returns {kind:'primary', priority:2}", () => {
    expect(primary(2)).toEqual({ kind: "primary", priority: 2 });
  });

  it("interop() returns bare {kind:'interop'} when priority omitted", () => {
    expect(interop()).toEqual({ kind: "interop" });
  });

  it("interop(3) returns {kind:'interop', priority:3}", () => {
    // Frozen row from T-B2-claims brief.
    expect(interop(3)).toEqual({ kind: "interop", priority: 3 });
  });

  it("fallback() returns bare {kind:'fallback'} when priority omitted", () => {
    expect(fallback()).toEqual({ kind: "fallback" });
  });
});
