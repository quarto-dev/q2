/**
 * @quarto/engine-host-deno — Deno entry point.
 *
 * This is the ONLY file in the package that touches Deno.* APIs.
 * It is excluded from tsconfig.json and the vitest test runner.
 *
 * Typecheck with:   deno check ts-packages/quarto-engine-host-deno/src/main.ts
 * Bundle with:      esbuild (Phase 4 — not yet).
 *
 * `runHost` manages the protocol loop; `Deno.exit(0)` is called here
 * (not in runHost) so the loop itself stays platform-neutral.
 *
 * `Deno.stdout` satisfies the `FrameWriter` interface:
 *   { write(bytes: Uint8Array): Promise<number> }
 * because Deno's stdout WritableStream writer is compatible.
 */
import { runHost } from "./host.ts";
import { denoHost } from "./deno-host.ts";

if (import.meta.main) {
  await runHost(Deno.stdin.readable, Deno.stdout, denoHost);
  Deno.exit(0);
}
