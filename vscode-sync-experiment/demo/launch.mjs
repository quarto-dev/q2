/** Entry point of `npm run demo`: runs the TypeScript launcher under tsx with the `source` export condition. */
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const tsx = require.resolve('tsx'); // loaded with --import, not via the tsx CLI: its extra process can exit before the runner finishes tearing down
const child = spawn(process.execPath, ['--conditions=source', '--import', pathToFileURL(tsx).href, join(here, 'src', 'cli.ts'), ...process.argv.slice(2)], { stdio: 'inherit' });

// A terminal Ctrl-C signals the whole process group, but `kill <pid>` (a script, an agent) reaches only this wrapper.
// Forward the signal so the runner always gets to tear down its windows and processes, and wait for it to finish.
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(sig, () => child.kill(sig));
child.on('exit', (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
