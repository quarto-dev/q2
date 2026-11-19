/** GE-11: the manifest declares the Join and Leave commands, and `quartoHub.q2Path` has machine scope (a workspace value is ignored). */
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, it } from 'vitest';

const manifest = JSON.parse(fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'package.json'), 'utf8'));

it('q2Path is a machine-scoped setting defaulting to q2', () => {
  const prop = manifest.contributes.configuration.properties['quartoHub.q2Path'];
  expect(prop.scope).toBe('machine');
  expect(prop.default).toBe('q2');
});

it('declares the join and leave commands and their activation events', () => {
  const commands = manifest.contributes.commands.map((c: { command: string }) => c.command);
  expect(commands).toEqual(expect.arrayContaining(['quartoHub.joinSession', 'quartoHub.leaveSession']));
  expect(manifest.activationEvents).toEqual(expect.arrayContaining(['onCommand:quartoHub.joinSession', 'onCommand:quartoHub.leaveSession']));
});
