/**
 * Render tool gating (CAP-12): rendering executes project code on the
 * operator's machine, so the `render` tool exists only when the operator
 * opted in with --allow-render. --read-only is the stricter posture and
 * wins the combination: code execution is beyond "look but don't touch".
 */

import { describe, it, expect } from 'vitest';

import { startInMemoryMcp } from './in-memory-fixture.js';

describe('render tool gating (CAP-12)', () => {
  it('is not listed without --allow-render', async () => {
    const f = await startInMemoryMcp();
    try {
      const tools = await f.client.listTools();
      expect(tools.tools.map((t) => t.name)).not.toContain('render');
    } finally {
      await f.close();
    }
  });

  it('is listed with --allow-render', async () => {
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      const tools = await f.client.listTools();
      const render = tools.tools.find((t) => t.name === 'render');
      expect(render).toBeDefined();
      // Honest annotations for a code-executing tool.
      expect(render?.annotations?.readOnlyHint).toBe(false);
      expect(render?.annotations?.openWorldHint).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('--read-only wins the combination: not listed even with --allow-render', async () => {
    const f = await startInMemoryMcp({ readOnly: true, allowRender: true });
    try {
      const tools = await f.client.listTools();
      expect(tools.tools.map((t) => t.name)).not.toContain('render');
    } finally {
      await f.close();
    }
  });
});
