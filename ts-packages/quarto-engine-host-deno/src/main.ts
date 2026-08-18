/**
 * @quarto/engine-host-deno — Deno entry point.
 *
 * This is one of a small set of files in the package that touch Deno.* APIs
 * (along with `deno-host.ts` and `control-transport.ts`). All three are
 * excluded from tsconfig.json and the vitest test runner.
 *
 * Typecheck with:   deno check ts-packages/quarto-engine-host-deno/src/main.ts
 * Bundle with:      esbuild — `npm run bundle -w @quarto/engine-host-deno`
 *   (or `cargo xtask build-engine-host-bundle`); the bundle is committed and
 *   embedded into the `quarto-core` Rust crate via `include_str!`.
 *
 * `runHost` manages the protocol loop; `Deno.exit(0)` is called here
 * (not in runHost) so the loop itself stays platform-neutral.
 *
 * Channel selection (plan1a.6 Phase 2 — "Deno dial-back"): when q2 passes
 * `--control <host:port>`, dial back over loopback TCP via `connectControl`
 * instead of using stdio. Phase 2 does NOT flip production — q2 does not yet
 * pass `--control`, so the stdio branch below stays the one every current
 * caller exercises. `Deno.stdout` satisfies the `FrameWriter` interface:
 *   { write(bytes: Uint8Array): Promise<number> }
 * because Deno's stdout WritableStream writer is compatible.
 */
import { runHost } from "./host.ts";
import { denoHost } from "./deno-host.ts";
import { connectControl } from "./control-transport.ts";

if (import.meta.main) {
  if (Deno.args.includes("--control")) {
    const { reader, writer } = await connectControl();
    await runHost(reader, writer, denoHost);
  } else {
    await runHost(Deno.stdin.readable, Deno.stdout, denoHost);
  }
  Deno.exit(0);
}
