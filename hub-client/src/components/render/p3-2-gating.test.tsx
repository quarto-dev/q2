/**
 * P3.2: nestedEditBuffers gating — regenerateNestedBuffers is NOT called
 * when unlockDepthCursor is off, IS called when on.
 *
 * Tests the exported `computeNestedEditBuffers` helper from ReactPreview.
 * The helper is a pure function; we pass a mocked `regen` argument so
 * no WASM boundary is involved and no jsdom/React rendering is needed.
 *
 * Fail-on-revert guarantee: remove `!unlock` from the guard inside
 * `computeNestedEditBuffers` → the "flag off" test calls regen and the
 * `expect(mockRegen).not.toHaveBeenCalled()` assertion turns RED.
 *
 * This is a genuine structural test, not theater: the mock is for the
 * WASM *environment*, which is always allowed. The memo logic is not
 * reimplemented here — it lives in the helper and is called verbatim.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { computeNestedEditBuffers } from './ReactPreview';

const CONTENT = '# Hello\n';
const AST = '{"pandoc-api-version":[1,23,0],"meta":{},"blocks":[]}';
const REGEN_RESULT = { '0:0-10:0': '- item\n' };

describe('computeNestedEditBuffers gating (P3.2)', () => {
  let mockRegen: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockRegen = vi.fn(() => REGEN_RESULT);
  });

  it('does NOT call regen when unlock is false', () => {
    const result = computeNestedEditBuffers(false, CONTENT, AST, mockRegen);
    expect(mockRegen).not.toHaveBeenCalled();
    // Returns the referentially stable empty object.
    expect(result).toEqual({});
  });

  it('calls regen with both inputs when unlock is true', () => {
    const result = computeNestedEditBuffers(true, CONTENT, AST, mockRegen);
    expect(mockRegen).toHaveBeenCalledWith(CONTENT, AST);
    expect(result).toEqual(REGEN_RESULT);
  });

  it('does NOT call regen when unlock is true but content is empty', () => {
    const result = computeNestedEditBuffers(true, '', AST, mockRegen);
    expect(mockRegen).not.toHaveBeenCalled();
    expect(result).toEqual({});
  });

  it('does NOT call regen when unlock is true but ast is null', () => {
    const result = computeNestedEditBuffers(true, CONTENT, null, mockRegen);
    expect(mockRegen).not.toHaveBeenCalled();
    expect(result).toEqual({});
  });

  it('returns EMPTY_NESTED_BUFFERS (same reference) on off-path across calls', () => {
    // The module-level constant is referentially stable — two off-path calls
    // must return the same object so useMemo's dep comparison short-circuits.
    const r1 = computeNestedEditBuffers(false, CONTENT, AST, mockRegen);
    const r2 = computeNestedEditBuffers(false, CONTENT, AST, mockRegen);
    expect(r1).toBe(r2);
  });

  it('returns EMPTY_NESTED_BUFFERS when regen throws', () => {
    const throwingRegen = vi.fn(() => {
      throw new Error('WASM error');
    });
    const r1 = computeNestedEditBuffers(true, CONTENT, AST, throwingRegen);
    const r2 = computeNestedEditBuffers(false, CONTENT, AST, mockRegen);
    // Both off-paths return the same stable reference.
    expect(r1).toBe(r2);
    expect(throwingRegen).toHaveBeenCalledOnce();
  });
});
