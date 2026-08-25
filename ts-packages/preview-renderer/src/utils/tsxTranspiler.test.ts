import { describe, it, expect } from 'vitest';
import { transpileTSX } from './tsxTranspiler';

/**
 * Tests for the shared TSX transpiler (moved from
 * `hub-client/src/services/tsxTranspiler.ts` in GH #402 / bd-ue80chl0
 * Phase 1). These exercise the real `@babel/standalone` transform —
 * consumers that don't want to pay for babel at test time mock this
 * module instead (see hub-client's `ReactRenderer.integration.test.tsx`
 * and the SPA's customComponents tests).
 */
describe('transpileTSX', () => {
  it('strips TypeScript and lowers JSX to React.createElement', () => {
    const tsx = [
      `export function Para({ node }: { node: unknown }) {`,
      `  return <p className="my-para">hi</p>;`,
      `}`,
    ].join('\n');
    const js = transpileTSX(tsx);
    // JSX lowered to the classic runtime (the iframe provides a global
    // `React` before importing the blob module).
    expect(js).toContain('React.createElement');
    expect(js).toContain('"my-para"');
    // Type annotations gone.
    expect(js).not.toContain(': unknown');
    // ESM export preserved — the iframe imports the code as a blob ES
    // module and reads the named exports.
    expect(js).toContain('export function Para');
  });

  it('throws a descriptive error on syntactically invalid input', () => {
    expect(() => transpileTSX('const = <p>;')).toThrow(/Failed to transpile TSX/);
  });
});
