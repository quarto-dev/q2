import { describe, expect, it } from "vitest";
import { MIGRATION_DOC, rewriteDenoError } from "./deno-diagnostic.js";

describe("rewriteDenoError", () => {
  it("rewrites a load-time failure naming the known extension path", () => {
    const out = rewriteDenoError(new ReferenceError("Deno is not defined"), "/ext/engine.js");
    expect(out?.message).toContain("engine extension /ext/engine.js");
    expect(out?.message).toContain("node:");
    expect(out?.message).toContain(MIGRATION_DOC);
    expect(out?.message).toContain("Original error: Deno is not defined");
  });

  it("recovers the extension path from a file:// stack frame", () => {
    const err = new ReferenceError("Deno is not defined");
    err.stack = "ReferenceError: Deno is not defined\n    at run (file:///ext/my%20engine/engine.js:10:5)";
    expect(rewriteDenoError(err)?.message).toContain("engine extension /ext/my engine/engine.js");
  });

  it("recovers an absolute path from a plain stack frame", () => {
    const err = new ReferenceError("Deno is not defined");
    err.stack = "ReferenceError: Deno is not defined\n    at /ext/engine.js:3:9";
    expect(rewriteDenoError(err)?.message).toContain("engine extension /ext/engine.js");
  });

  it("still rewrites when no path can be found", () => {
    const err = new ReferenceError("Deno is not defined");
    err.stack = "";
    expect(rewriteDenoError(err)?.message).toContain("an engine extension uses");
  });

  it("ignores every other error", () => {
    expect(rewriteDenoError(new Error("boom"))).toBeUndefined();
    expect(rewriteDenoError(new ReferenceError("Denominator is not defined"))).toBeUndefined();
    expect(rewriteDenoError("Deno.readTextFileSync is not a function")).toBeUndefined();
  });
});
