/**
 * Tests for the Node control-channel dial-back (`node-control-transport.ts`)
 * against a real `node:net` server on loopback.
 */
import { afterEach, describe, expect, it } from "vitest";
import { createServer, type Server, type Socket } from "node:net";
import { connectControl } from "./node-control-transport.js";

const servers: Server[] = [];
afterEach(() => {
  for (const server of servers.splice(0)) server.close();
});

/** A listener that hands each accepted socket to `onSocket`. */
async function listen(onSocket: (socket: Socket) => void): Promise<number> {
  const server = createServer(onSocket);
  servers.push(server);
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return (server.address() as { port: number }).port;
}

/** A stdin stand-in that serves `text` one byte per read, then EOF. */
function fakeStdin(text: string) {
  const bytes = new TextEncoder().encode(text);
  let offset = 0;
  const reads: number[] = [];
  return {
    reads,
    readStdin(buf: Uint8Array): number {
      if (offset >= bytes.length) return 0;
      buf[0] = bytes[offset++];
      reads.push(1);
      return 1;
    },
    consumed: () => offset,
  };
}

const collect = async (stream: ReadableStream<Uint8Array>, bytes: number) => {
  const chunks: Uint8Array[] = [];
  let total = 0;
  const reader = stream.getReader();
  while (total < bytes) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    total += value.length;
  }
  reader.releaseLock();
  return Buffer.concat(chunks);
};

describe("connectControl", () => {
  it("writes the token first, then exchanges bytes in both directions", async () => {
    const received: Buffer[] = [];
    const port = await listen((socket) => {
      socket.on("data", (chunk) => {
        received.push(chunk);
        socket.write("pong");
      });
    });
    const stdin = fakeStdin("secret-token\nIGNORED");
    const { reader, writer } = await connectControl({
      args: ["--control", `127.0.0.1:${port}`],
      readStdin: stdin.readStdin,
    });
    // Never consumes a byte past the newline.
    expect(stdin.consumed()).toBe("secret-token\n".length);

    expect(await writer.write(new TextEncoder().encode("ping"))).toBe(4);
    expect((await collect(reader, 4)).toString()).toBe("pong");
    await writer.end();
    expect(Buffer.concat(received).toString()).toBe("secret-token\nping");
  });

  it("delivers a large write completely (backpressure)", async () => {
    const size = 5 * 1024 * 1024;
    let total = 0;
    let finished!: () => void;
    const done = new Promise<void>((resolve) => (finished = resolve));
    const port = await listen((socket) => {
      socket.on("data", (chunk) => {
        total += chunk.length;
      });
      socket.on("end", finished);
    });
    const { writer } = await connectControl({
      args: ["--control", `127.0.0.1:${port}`],
      readStdin: fakeStdin("t\n").readStdin,
    });
    await writer.write(new Uint8Array(size));
    await writer.end();
    await done;
    expect(total).toBe(size + 2);
  });

  it("surfaces the server closing the connection as end of stream", async () => {
    const port = await listen((socket) => {
      socket.on("data", () => socket.end());
    });
    const { reader } = await connectControl({
      args: ["--control", `127.0.0.1:${port}`],
      readStdin: fakeStdin("t\n").readStdin,
    });
    const { done } = await reader.getReader().read();
    expect(done).toBe(true);
  });

  it("rejects pending writes when the socket errors", async () => {
    const port = await listen((socket) => {
      socket.on("data", () => socket.resetAndDestroy());
    });
    const { writer } = await connectControl({
      args: ["--control", `127.0.0.1:${port}`],
      readStdin: fakeStdin("t\n").readStdin,
    });
    await new Promise((resolve) => setTimeout(resolve, 100));
    await expect(
      (async () => {
        for (let i = 0; i < 50; i++) {
          await writer.write(new Uint8Array(65536));
        }
      })(),
    ).rejects.toThrow();
  });

  it("treats EOF before a newline as the whole token", async () => {
    const received: Buffer[] = [];
    const port = await listen((socket) => socket.on("data", (c) => received.push(c)));
    const { writer } = await connectControl({
      args: ["--control", `127.0.0.1:${port}`],
      readStdin: fakeStdin("partial").readStdin,
    });
    await writer.end();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(Buffer.concat(received).toString()).toBe("partial\n");
  });

  it("rejects a missing or malformed --control argument", async () => {
    const deps = { readStdin: fakeStdin("t\n").readStdin };
    await expect(connectControl({ ...deps, args: [] })).rejects.toThrow("not found");
    await expect(connectControl({ ...deps, args: ["--control"] })).rejects.toThrow("not found");
    await expect(connectControl({ ...deps, args: ["--control", "noport"] })).rejects.toThrow("malformed");
    await expect(connectControl({ ...deps, args: ["--control", "h:abc"] })).rejects.toThrow("malformed port");
  });

  it("rejects when nothing is listening", async () => {
    const port = await listen(() => {});
    servers.pop()!.close();
    await expect(
      connectControl({
        args: ["--control", `127.0.0.1:${port}`],
        readStdin: fakeStdin("t\n").readStdin,
      }),
    ).rejects.toMatchObject({ code: "ECONNREFUSED" });
  });
});
