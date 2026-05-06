import { describe, it, expect } from 'vitest';
import { pipelineKindForFormat } from './pipelineKind';

// q2-preview Plan 1 commit 6: the format → pipeline-kind mapping is
// the single source of truth on the JS side for which Quarto pipeline
// drives a render. ReactPreview.doRender's data-source switch reads
// through this; Plan 7's edit-back wiring will too. A regression here
// silently breaks q2-preview routing without surfacing as a test
// failure anywhere else.
describe('pipelineKindForFormat', () => {
  it('returns "preview" for q2-preview', () => {
    expect(pipelineKindForFormat('q2-preview')).toBe('preview');
  });

  it('returns undefined for q2-debug (parser-only path, not the q2-preview pipeline)', () => {
    expect(pipelineKindForFormat('q2-debug')).toBeUndefined();
  });

  it('returns undefined for q2-slides today (Plan 1 Decision A migrates this later)', () => {
    expect(pipelineKindForFormat('q2-slides')).toBeUndefined();
  });

  it('returns undefined for plain html', () => {
    expect(pipelineKindForFormat('html')).toBeUndefined();
  });

  it('returns undefined for unknown formats', () => {
    expect(pipelineKindForFormat('not-a-real-format')).toBeUndefined();
    expect(pipelineKindForFormat('')).toBeUndefined();
  });
});
