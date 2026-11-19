#!/usr/bin/env node
// Prototype launcher for a custom editor on top of `q2 preview`.
//
//   node scripts/q2-preview-custom.mjs [q2 preview args...]
//   node scripts/q2-preview-custom.mjs docs/ --allow-edit
//
// Runs `q2 preview --ui custom`, reads the JSON line of server URLs it
// prints, then starts the Vite dev server for q2-demos/custom-editor with
// those URLs as VITE_Q2_* env vars and opens it in the browser.
//
// Uses target/debug/q2 (build it with `cargo build --bin q2`); set Q2_BIN
// to use another binary.

import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const exe = process.platform === 'win32' ? 'q2.exe' : 'q2';
const q2 = process.env.Q2_BIN ?? path.join(root, 'target', 'debug', exe);

const preview = spawn(q2, ['preview', ...process.argv.slice(2), '--ui', 'custom'], {
  stdio: ['inherit', 'pipe', 'inherit'],
});

let vite;
const shutdown = async () => {
  preview.kill();
  await vite?.close();
  process.exit();
};
preview.on('error', (error) => {
  console.error(`could not run ${q2}: ${error.message}`);
  process.exit(1);
});
preview.on('exit', shutdown);
process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);

for await (const line of createInterface({ input: preview.stdout })) {
  if (vite !== undefined || !line.startsWith('{')) {
    console.log(line);
    continue;
  }
  const info = JSON.parse(line);
  console.log('q2 preview:', info);
  process.env.VITE_Q2_WS_URL = info.wsUrl;
  process.env.VITE_Q2_INDEX_DOC_ID = info.indexDocId;
  process.env.VITE_Q2_ALLOW_EDIT = String(info.allowEdit);
  if (info.file !== null) process.env.VITE_Q2_FILE = info.file;

  vite = await createServer({
    root: path.join(root, 'q2-demos', 'custom-editor'),
    server: { open: true },
  });
  await vite.listen();
  vite.printUrls();
}
