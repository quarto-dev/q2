/** Entry point of `npm run demo:style-loop`: runs the TypeScript loop under tsx with the `source` export condition. */
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const tsx = createRequire(import.meta.url).resolve('tsx');
const child = spawn(process.execPath, ['--conditions=source', '--import', pathToFileURL(tsx).href, join(here, 'src', 'style-loop-run.ts'), ...process.argv.slice(2)], { stdio: 'inherit' });
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(sig, () => child.kill(sig));
child.on('exit', (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
