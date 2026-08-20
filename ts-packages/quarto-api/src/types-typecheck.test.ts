/**
 * Typecheck test: verifies that @quarto/types can be imported and its types
 * are usable. Named revert that makes this RED: dropping `Format` from
 * the vendored quarto-types/src/format.ts (or from the index re-exports).
 */

import { describe, it, expect } from "vitest";
import type { Format } from "@quarto/types";

describe("@quarto/types typecheck", () => {
  it("imports a real exported type (Format) from @quarto/types and uses it as a type annotation", () => {
    // This test's value is in compilation: if `Format` is not exported by
    // @quarto/types, this file will fail to typecheck at all.
    // At runtime, we just verify the import resolved (the type is erased, but
    // we can use it in a runtime-callable way via a type-only assertion helper).
    const asFormat = (x: Format): Format => x;
    // Create a minimal object that satisfies the Format interface
    // (we only need it to compile; we don't exercise all fields)
    const partial = {} as Format;
    expect(asFormat(partial)).toBe(partial);
  });
});
