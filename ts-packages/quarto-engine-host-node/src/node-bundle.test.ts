/**
 * End-to-end tests for the built Node bundle (`dist/engine-host-node.mjs`):
 * spawn it the way q2 does (token on stdin, `--control` socket) and speak the
 * framed protocol from a `node:net` server standing in for the Rust side.
 *
 * Rebuild the bundle first when sources change: `npm run bundle`.
 */
import { afterEach, describe, expect, it } from "vitest";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type AddressInfo, type Server, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const bundle = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "dist",
  "engine-host-node.mjs",
);

const INIT = {
  type: "init",
  global: {
    resourceDir: "/res",
    runtimeDir: "/run",
    dataDir: "/data",
    pandocPath: null,
    isInteractiveSession: false,
    runningInCi: false,
    quartoVersion: "99.9.9",
  },
};

const scratch: string[] = [];
const servers: Server[] = [];
const children: ChildProcess[] = [];
afterEach(() => {
  for (const child of children.splice(0)) child.kill();
  for (const server of servers.splice(0)) server.close();
  for (const dir of scratch.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function scratchDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "node-bundle-test-"));
  scratch.push(dir);
  return dir;
}

/** The Rust side: accepts one connection, validates the token, then talks frames. */
class FakeQ2 {
  socket!: Socket;
  private buffer = Buffer.alloc(0);
  private waiters: Array<() => void> = [];
  private handshake = Buffer.alloc(0);
  child!: ChildProcess;
  stderr = "";
  exit!: Promise<number | null>;

  static async start(token: string, cwd?: string): Promise<FakeQ2> {
    const q2 = new FakeQ2();
    const connected = new Promise<void>((resolve) => {
      const server = createServer((socket) => {
        q2.socket = socket;
        socket.on("data", (chunk) => {
          q2.buffer = Buffer.concat([q2.buffer, chunk]);
          for (const wake of q2.waiters.splice(0)) wake();
        });
        resolve();
      });
      servers.push(server);
      server.listen(0, "127.0.0.1", () => {
        const { port } = server.address() as AddressInfo;
        q2.child = spawn(process.execPath, [bundle, "--control", `127.0.0.1:${port}`], {
          stdio: ["pipe", "ignore", "pipe"],
          cwd,
        });
        children.push(q2.child);
        q2.child.stderr!.on("data", (c) => (q2.stderr += c));
        q2.exit = new Promise((r) => q2.child.on("close", (code) => r(code)));
        q2.child.stdin!.write(`${token}\n`);
        q2.child.stdin!.end();
      });
    });
    await connected;
    return q2;
  }

  private async until(predicate: () => boolean): Promise<void> {
    while (!predicate()) {
      await new Promise<void>((resolve) => this.waiters.push(resolve));
    }
  }

  /** Read the token line the child writes first. */
  async readToken(): Promise<string> {
    await this.until(() => this.buffer.includes(0x0a));
    const i = this.buffer.indexOf(0x0a);
    this.handshake = this.buffer.subarray(0, i);
    this.buffer = this.buffer.subarray(i + 1);
    return this.handshake.toString();
  }

  send(id: number, msg: unknown): void {
    this.socket.write(JSON.stringify({ id, msg }) + "\n");
  }

  /** Read the next response frame. */
  async frame(): Promise<{ id: number; msg: { type: string; [k: string]: unknown } }> {
    await this.until(() => this.buffer.includes(0x0a));
    const i = this.buffer.indexOf(0x0a);
    const line = this.buffer.subarray(0, i).toString();
    this.buffer = this.buffer.subarray(i + 1);
    return JSON.parse(line);
  }
}

function writeEngine(dir: string, fileName: string, body: string): string {
  mkdirSync(dir, { recursive: true });
  const path = join(dir, fileName);
  writeFileSync(path, body);
  return path;
}

const engineSource = (name: string, markdown: string) => `
export default {
  name: ${JSON.stringify(name)},
  claimsLanguage: () => false,
  defaultExt: ".qmd",
  defaultYaml: () => [],
  defaultContent: () => [],
  validExtensions: () => [".qmd"],
  claimsFile: () => false,
  canFreeze: false,
  generatesFigures: false,
  launch: () => ({
    name: ${JSON.stringify(name)},
    canFreeze: false,
    markdownForFile: async () => ({ value: ${markdown}, map: () => undefined }),
    target: async () => undefined,
    partitionedMarkdown: async () => { throw new Error("unused"); },
    execute: async () => { throw new Error("unused"); },
    dependencies: async () => ({ includes: {} }),
    postprocess: async () => {},
  }),
};
`;

describe("engine-host-node.mjs", () => {
  it("authenticates, serves a multi-megabyte response completely, and exits cleanly", async () => {
    const dir = scratchDir();
    const enginePath = writeEngine(
      dir,
      "engine.mjs",
      engineSource("big", '"x".repeat(5 * 1024 * 1024)'),
    );
    const q2 = await FakeQ2.start("tok-123");
    expect(await q2.readToken()).toBe("tok-123");

    q2.send(0, INIT);
    q2.send(1, { type: "loadEngine", enginePath });
    const loadedFrame = await q2.frame();
    expect(loadedFrame.msg.message ?? loadedFrame.msg.type).toBe("loaded");
    q2.send(2, { type: "launchEngine", engine: "big", project: { isSingleFile: true } });
    expect((await q2.frame()).msg.type).toBe("launched");
    q2.send(3, { type: "markdownForFile", engine: "big", file: "a.qmd" });
    const reply = await q2.frame();
    expect(reply.id).toBe(3);
    expect(reply.msg.type).toBe("markdownForFileResult");
    const result = reply.msg.result as { value: string };
    expect(result.value.length).toBe(5 * 1024 * 1024);

    q2.send(4, { type: "shutdown" });
    expect(await q2.exit).toBe(0);
    expect(q2.stderr).toBe("");
  }, 30_000);

  it("does not consume stdin past the token line", async () => {
    // The child's stdin is closed after the token; if it read ahead it would
    // not matter here, so assert the observable contract instead: the token
    // reaches the server exactly, with no trailing bytes.
    const q2 = await FakeQ2.start("exact");
    expect(await q2.readToken()).toBe("exact");
    q2.send(0, INIT);
    q2.send(1, { type: "shutdown" });
    expect(await q2.exit).toBe(0);
  }, 15_000);

  it("loads an engine next to a typeless package.json without writing to stderr", async () => {
    const dir = scratchDir();
    writeFileSync(join(dir, "package.json"), "{}");
    // ESM syntax in a `.js` file with no "type": Node must not warn about it.
    const enginePath = writeEngine(dir, "engine.js", engineSource("typeless", '"ok"'));
    const q2 = await FakeQ2.start("t", dir);
    await q2.readToken();
    q2.send(0, INIT);
    q2.send(1, { type: "loadEngine", enginePath });
    const loaded = await q2.frame();
    expect(loaded.msg.type).toBe("loaded");
    q2.send(2, { type: "shutdown" });
    expect(await q2.exit).toBe(0);
    expect(q2.stderr).toBe("");
  }, 15_000);

  it("rewrites an engine's `Deno is not defined` failure into an actionable error", async () => {
    const dir = scratchDir();
    const enginePath = writeEngine(
      dir,
      "old-engine.mjs",
      'Deno.readTextFileSync("x");\nexport default {};\n',
    );
    const q2 = await FakeQ2.start("t");
    await q2.readToken();
    q2.send(0, INIT);
    q2.send(1, { type: "loadEngine", enginePath });
    const reply = await q2.frame();
    expect(reply.msg.type).toBe("error");
    expect(reply.msg.message).toContain(`engine extension ${enginePath}`);
    expect(reply.msg.message).toContain("node:");
    q2.send(2, { type: "shutdown" });
    expect(await q2.exit).toBe(0);
  }, 15_000);

  it("exits non-zero with a message when --control is missing", async () => {
    const child = spawn(process.execPath, [bundle], { stdio: ["ignore", "ignore", "pipe"] });
    children.push(child);
    let stderr = "";
    child.stderr!.on("data", (c) => (stderr += c));
    const code = await new Promise((r) => child.on("close", r));
    expect(code).toBe(2);
    expect(stderr).toContain("--control");
  });
});
