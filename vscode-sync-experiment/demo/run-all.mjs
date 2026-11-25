/** Entry point of `npm run demo:all`: runs the TypeScript sequence runner under tsx with the `source` export condition. */
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const tsx = require.resolve('tsx'); // loaded with --import, as `launch.mjs` does
const child = spawn(process.execPath, ['--conditions=source', '--import', pathToFileURL(tsx).href, join(here, 'src', 'run-all.ts'), ...process.argv.slice(2)], { stdio: 'inherit' });

// Forward signals so the runner can stop the story that is running and tear it down, and wait for it to finish.
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(sig, () => child.kill(sig));
child.on('exit', (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
