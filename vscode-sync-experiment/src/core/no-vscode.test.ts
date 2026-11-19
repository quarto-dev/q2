import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, it } from 'vitest';

it('nothing under src/core imports vscode', async () => {
  const dir = path.dirname(fileURLToPath(import.meta.url));
  const offenders: string[] = [];
  for (const name of await fs.readdir(dir)) {
    if (!name.endsWith('.ts') || name === 'no-vscode.test.ts') continue;
    const text = await fs.readFile(path.join(dir, name), 'utf8');
    if (/from\s+['"]vscode['"]|require\(\s*['"]vscode['"]\s*\)|import\(\s*['"]vscode['"]\s*\)/.test(text)) offenders.push(name);
  }
  expect(offenders).toEqual([]);
});
