import { describe, it, expect } from 'vitest';
import { projectFolderName } from './project-folder-name.js';

describe('projectFolderName', () => {
  it('turns spaces into hyphens (existing download-filename behavior)', () => {
    expect(projectFolderName('Demo Playground')).toBe('Demo-Playground');
  });

  it('falls back to "project" for undefined or empty names', () => {
    expect(projectFolderName(undefined)).toBe('project');
    expect(projectFolderName('')).toBe('project');
    expect(projectFolderName('   ')).toBe('project');
  });

  it('collapses path separators into a single safe segment', () => {
    expect(projectFolderName('a/b')).toBe('a-b');
    expect(projectFolderName('a\\b')).toBe('a-b');
  });

  it('replaces Windows-hostile characters rather than preserving them', () => {
    const result = projectFolderName('A: B? "C" <D> |E| *F*');
    expect(result).not.toMatch(/[<>:"/\\|?*]/);
    // spaces and reserved chars collapse to single hyphens
    expect(result).toBe('A-B-C-D-E-F');
  });

  it('replaces control characters', () => {
    // Tab (U+0009) and other C0 control chars must not survive into a path.
    const tab = String.fromCharCode(9);
    const result = projectFolderName('a' + tab + 'b' + tab + 'c');
    expect(result).toBe('a-b-c');
  });

  it('strips trailing dots and spaces (illegal on Windows)', () => {
    expect(projectFolderName('My Project.')).toBe('My-Project');
    expect(projectFolderName('My Project...')).toBe('My-Project');
    expect(projectFolderName('trailing ')).toBe('trailing');
  });

  it('trims leading/trailing hyphens produced by surrounding slashes', () => {
    expect(projectFolderName('/My Project/')).toBe('My-Project');
    expect(projectFolderName('/leading')).toBe('leading');
  });

  it('returns a non-empty result even for all-hostile input', () => {
    expect(projectFolderName('///')).toBe('project');
    expect(projectFolderName('***')).toBe('project');
    expect(projectFolderName('...')).toBe('project');
  });
});
