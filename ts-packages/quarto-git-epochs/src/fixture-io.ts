/** Test helper: loads the shared fixtures. Rows are skipped by name, never by position. */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), '..', 'fixtures');
export const readJson = (rel: string): any => JSON.parse(readFileSync(join(FIXTURES, rel), 'utf8'));
