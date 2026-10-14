/**
 * Tests for the Node PlatformHost (`node-host.ts`). Runs under vitest on Node.
 */
import { afterEach, describe, expect, it } from "vitest";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import process from "node:process";
import { nodeHost } from "./node-host.js";

const scratch: string[] = [];
function scratchDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "node-host-test-"));
  scratch.push(dir);
  return dir;
}
afterEach(() => {
  for (const dir of scratch.splice(0)) rmSync(dir, { recursive: true, force: true });
});

const node = (script: string, ...args: string[]) =>
  nodeHost.process.exec(process.execPath, ["-e", script, ...args]);

describe("fs", () => {
  it("round-trips text and bytes, and reports existence", () => {
    const dir = scratchDir();
    const file = join(dir, "a.txt");
    expect(nodeHost.fs.exists(file)).toBe(false);
    nodeHost.fs.writeFileSync(file, "héllo");
    expect(nodeHost.fs.exists(file)).toBe(true);
    expect(nodeHost.fs.readTextFileSync(file)).toBe("héllo");
    nodeHost.fs.writeFileSync(file, new Uint8Array([0x68, 0x69]));
    expect(nodeHost.fs.readTextFileSync(file)).toBe("hi");
  });

  it("ensureDir is recursive and idempotent", () => {
    const dir = join(scratchDir(), "a", "b");
    nodeHost.fs.ensureDir(dir);
    nodeHost.fs.ensureDir(dir);
    expect(statSync(dir).isDirectory()).toBe(true);
  });

  it("makeTempDir honours prefix and dir", () => {
    const parent = scratchDir();
    const made = nodeHost.fs.makeTempDir({ prefix: "pre-", dir: parent });
    expect(dirname(made)).toBe(parent);
    expect(basename(made).startsWith("pre-")).toBe(true);
    expect(statSync(made).isDirectory()).toBe(true);
    // With no options the directory must land INSIDE the temp dir, not beside
    // it (`mkdtemp("/tmp")` would create `/tmpXXXXXX`; macOS hid that because
    // its `tmpdir()` ends in a slash).
    const dflt = nodeHost.fs.makeTempDir();
    expect(statSync(dflt).isDirectory()).toBe(true);
    expect(dirname(dflt)).toBe(resolve(tmpdir()));
  });

  it("makeTempFile honours prefix and suffix and creates distinct files", () => {
    const parent = scratchDir();
    const a = nodeHost.fs.makeTempFile({ prefix: "p-", suffix: ".html", dir: parent });
    const b = nodeHost.fs.makeTempFile({ prefix: "p-", suffix: ".html", dir: parent });
    expect(a).not.toBe(b);
    for (const file of [a, b]) {
      expect(dirname(file)).toBe(parent);
      expect(basename(file).startsWith("p-")).toBe(true);
      expect(file.endsWith(".html")).toBe(true);
      expect(statSync(file).isFile()).toBe(true);
    }
  });

  it("remove deletes files, and directories only when recursive", () => {
    const dir = scratchDir();
    const file = join(dir, "f");
    writeFileSync(file, "x");
    nodeHost.fs.remove(file);
    expect(existsSync(file)).toBe(false);

    const tree = join(dir, "tree");
    mkdirSync(join(tree, "sub"), { recursive: true });
    writeFileSync(join(tree, "sub", "g"), "x");
    expect(() => nodeHost.fs.remove(tree)).toThrow();
    nodeHost.fs.remove(tree, { recursive: true });
    expect(existsSync(tree)).toBe(false);
  });

  describe("walk", () => {
    function tree(): string {
      const root = scratchDir();
      mkdirSync(join(root, "d1", "d2"), { recursive: true });
      writeFileSync(join(root, "top.txt"), "");
      writeFileSync(join(root, "d1", "mid.txt"), "");
      writeFileSync(join(root, "d1", "d2", "deep.txt"), "");
      return root;
    }
    const names = (root: string, entries: { path: string }[]) =>
      entries.map((e) => e.path.slice(root.length + 1)).sort();

    it("lists files only by default and never the root", () => {
      const root = tree();
      const entries = nodeHost.fs.walk(root);
      expect(names(root, entries)).toEqual(["d1/d2/deep.txt", "d1/mid.txt", "top.txt"]);
      expect(entries.every((e) => e.isFile && !e.isDirectory)).toBe(true);
    });

    it("includes directories (root first) when asked", () => {
      const root = tree();
      const entries = nodeHost.fs.walk(root, { includeDirs: true });
      expect(entries[0]).toEqual({ path: root, isFile: false, isDirectory: true });
      expect(names(root, entries.slice(1))).toEqual([
        "d1", "d1/d2", "d1/d2/deep.txt", "d1/mid.txt", "top.txt",
      ]);
    });

    it("respects maxDepth (1 = children of the root)", () => {
      const root = tree();
      expect(names(root, nodeHost.fs.walk(root, { maxDepth: 1 }))).toEqual(["top.txt"]);
      expect(names(root, nodeHost.fs.walk(root, { maxDepth: 2 }))).toEqual([
        "d1/mid.txt", "top.txt",
      ]);
    });
  });
});

describe("process.exec", () => {
  it("captures stdout, stderr and the exit code", async () => {
    const result = await node(
      "process.stdout.write('out'); process.stderr.write('err'); process.exit(3)",
    );
    expect(result).toEqual({ code: 3, success: false, stdout: "out", stderr: "err" });
  });

  it("reports success for exit code 0", async () => {
    const result = await node("process.stdout.write('ok')");
    expect(result).toMatchObject({ code: 0, success: true, stdout: "ok" });
  });

  it("feeds stdin", async () => {
    const result = await nodeHost.process.exec(
      process.execPath,
      ["-e", "process.stdin.pipe(process.stdout)"],
      { stdin: "from stdin ✓" },
    );
    expect(result.stdout).toBe("from stdin ✓");
  });

  it("closes stdin when none is given", async () => {
    const result = await node("process.stdin.on('end', () => console.log('eof')).resume()");
    expect(result.stdout.trim()).toBe("eof");
  });

  it("merges the parent environment with opts.env", async () => {
    process.env.NODE_HOST_TEST_PARENT = "parent";
    try {
      const result = await nodeHost.process.exec(
        process.execPath,
        ["-e", "console.log(process.env.NODE_HOST_TEST_PARENT + ':' + process.env.NODE_HOST_TEST_CHILD)"],
        { env: { NODE_HOST_TEST_CHILD: "child" } },
      );
      expect(result.stdout.trim()).toBe("parent:child");
    } finally {
      delete process.env.NODE_HOST_TEST_PARENT;
    }
  });

  it("runs in opts.cwd", async () => {
    const dir = scratchDir();
    const result = await nodeHost.process.exec(
      process.execPath,
      ["-e", "console.log(process.cwd())"],
      { cwd: dir },
    );
    expect(result.stdout.trim()).toBe(nodeHost.realPath(dir));
  });

  it("mergeOutput stderr>stdout puts everything in stdout", async () => {
    const result = await nodeHost.process.exec(
      process.execPath,
      ["-e", "process.stdout.write('A'); setTimeout(() => process.stderr.write('B'), 50)"],
      { mergeOutput: "stderr>stdout" },
    );
    expect(result.stdout).toBe("AB");
    expect(result.stderr).toBe("");
  });

  it("mergeOutput stdout>stderr puts everything in stderr", async () => {
    const result = await nodeHost.process.exec(
      process.execPath,
      ["-e", "process.stdout.write('A'); setTimeout(() => process.stderr.write('B'), 50)"],
      { mergeOutput: "stdout>stderr" },
    );
    expect(result.stderr).toBe("AB");
    expect(result.stdout).toBe("");
  });

  it("stderrFilter rewrites only stderr", async () => {
    const result = await nodeHost.process.exec(
      process.execPath,
      ["-e", "process.stdout.write('keep'); process.stderr.write('drop me')"],
      { stderrFilter: (text) => text.toUpperCase() },
    );
    expect(result).toMatchObject({ stdout: "keep", stderr: "DROP ME" });
  });

  it("decodes multi-byte characters split across chunks", async () => {
    const result = await node(
      "const b = Buffer.from('日本'); process.stdout.write(b.subarray(0, 4)); setTimeout(() => process.stdout.write(b.subarray(4)), 30)",
    );
    expect(result.stdout).toBe("日本");
  });

  it("times out, kills the child, and rejects", async () => {
    const started = Date.now();
    await expect(
      nodeHost.process.exec(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {
        timeout: 100,
      }),
    ).rejects.toThrow("Process timed out");
    expect(Date.now() - started).toBeLessThan(5000);
  });

  it("rejects when the command does not exist", async () => {
    await expect(
      nodeHost.process.exec("node-host-test-no-such-command", []),
    ).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("tolerates a child that exits without reading stdin", async () => {
    const result = await nodeHost.process.exec(process.execPath, ["-e", ""], {
      stdin: "x".repeat(1 << 20),
    });
    expect(result.success).toBe(true);
  });
});

describe("process lifecycle and identity", () => {
  it("onExit registers a synchronous handler on the process 'exit' event", () => {
    const before = process.listeners("exit");
    nodeHost.process.onExit(() => {});
    expect(process.listenerCount("exit")).toBe(before.length + 1);
    process.removeAllListeners("exit");
    for (const listener of before) process.on("exit", listener);
  });

  it("onExit runs its handler when a child process exits", async () => {
    const result = await node(
      "const h = require('node:fs'); process.on('exit', () => h.writeSync(1, 'bye'));",
    );
    expect(result.stdout).toBe("bye");
  });

  it("exposes cwd, env, realPath and CI/TTY flags", () => {
    expect(nodeHost.cwd()).toBe(process.cwd());
    process.env.NODE_HOST_TEST_VAR = "v";
    expect(nodeHost.env.get("NODE_HOST_TEST_VAR")).toBe("v");
    delete process.env.NODE_HOST_TEST_VAR;
    expect(nodeHost.env.get("NODE_HOST_TEST_VAR")).toBeUndefined();
    const file = join(scratchDir(), "f");
    writeFileSync(file, "");
    expect(nodeHost.realPath(file)).toBe(realpathSync(file));
    expect(typeof nodeHost.isInteractive).toBe("boolean");
    expect(nodeHost.isCI).toBe(!!process.env.CI);
  });
});

