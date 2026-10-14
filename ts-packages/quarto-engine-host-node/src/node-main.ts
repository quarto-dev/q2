/**
 * @quarto/engine-host-node — Node entry point (control mode only).
 *
 * q2 spawns `node <bundle> --control <host:port>` and passes a one-time token
 * on stdin; the host dials back over loopback TCP and speaks the framed
 * protocol on that socket. There is no stdio mode and no `import.meta.main`
 * guard: the bundle is always the entry point.
 *
 * Exit: `process.exitCode` plus a drained socket, never `process.exit()` after
 * a large write — on pipes `process.exit` can truncate buffered output.
 */
import { runHost } from "./host.js";
import { nodeHost } from "./node-host.js";
import { connectControl } from "./node-control-transport.js";
import process from "node:process";

// Node warns on stderr when it loads ESM syntax from a `.js` file whose nearest
// package.json has no "type". An engine extension sitting in a user's project
// triggers that through no fault of its own, so drop just that warning.
const emitWarning = process.emitWarning;
process.emitWarning = ((warning: string | Error, ...args: unknown[]) => {
  const options = args[0] as { code?: string } | string | undefined;
  const code = typeof options === "object" ? options?.code : args[1];
  if (code === "MODULE_TYPELESS_PACKAGE_JSON") return;
  return (emitWarning as (...a: unknown[]) => void).call(process, warning, ...args);
}) as typeof process.emitWarning;

if (!process.argv.includes("--control")) {
  process.stderr.write("engine host: --control <host:port> is required\n");
  process.exit(2);
}

try {
  const { reader, writer } = await connectControl();
  await runHost(reader, writer, nodeHost);
  await writer.end();
  process.exitCode = 0;
} catch (error) {
  process.stderr.write(`engine host: ${(error as Error).stack ?? error}\n`);
  process.exitCode = 1;
}
// Let the event loop drain; engines may leave timers or sockets behind, so
// force the exit once the protocol loop has finished.
setImmediate(() => process.exit());
