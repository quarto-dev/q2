/**
 * @quarto/api — crypto namespace tests (PURE)
 *
 * Tests the md5Hash function with known digest vectors.
 * Named revert: replacing md5Hash body with `return content` → both tests RED.
 *
 * MD5 reference vectors from RFC 1321:
 *   md5("") = "d41d8cd98f00b204e9800998ecf8427e"
 *   md5("abc") = "900150983cd24fb0d6963f7d28e17f72"
 *   md5("The quick brown fox jumps over the lazy dog") = "9e107d9d372bb6826bd81d3542a419d6"
 */

import { describe, it, expect } from "vitest";
import { md5Hash } from "./index.js";

describe("crypto.md5Hash", () => {
  it("returns the known RFC 1321 digest for empty string", () => {
    // Revert body to `return content` → RED (would return "")
    expect(md5Hash("")).toBe("d41d8cd98f00b204e9800998ecf8427e");
  });

  it("returns the known digest for 'abc'", () => {
    // Revert body to `return content` → RED (would return "abc")
    expect(md5Hash("abc")).toBe("900150983cd24fb0d6963f7d28e17f72");
  });

  it("returns the known digest for the quick-brown-fox string", () => {
    expect(md5Hash("The quick brown fox jumps over the lazy dog")).toBe(
      "9e107d9d372bb6826bd81d3542a419d6",
    );
  });

  it("returns lowercase hex string of length 32", () => {
    const digest = md5Hash("hello world");
    expect(digest).toHaveLength(32);
    expect(digest).toMatch(/^[0-9a-f]{32}$/);
  });

  it("different inputs produce different digests (collision check)", () => {
    expect(md5Hash("foo")).not.toBe(md5Hash("bar"));
  });
});
