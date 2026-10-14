import assert from "node:assert/strict";
import { createServer, type Server, type Socket } from "node:net";
import { afterEach, describe, test } from "node:test";

import {
  buildRequestLine,
  connectReady,
  LineConnection,
  type ProgressUpdate,
  sendCommand,
  signPayload,
} from "../../src/transport.ts";

// A fake control server. `onConnection` gets each accepted socket.
const servers: Server[] = [];
const conns: LineConnection[] = [];
const sockets = new Set<Socket>();

async function fakeServer(
  onConnection: (socket: Socket) => void,
): Promise<number> {
  const server = createServer((s) => {
    sockets.add(s);
    s.on("error", () => {});
    onConnection(s);
  });
  servers.push(server);
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
  return (server.address() as { port: number }).port;
}

async function connectTo(port: number) {
  const conn = await LineConnection.connect(port);
  conns.push(conn);
  return conn;
}

afterEach(async () => {
  for (const c of conns.splice(0)) c.close();
  for (const s of servers.splice(0)) {
    for (const sock of sockets) sock.destroy();
    await new Promise((r) => s.close(r));
  }
});

const pause = (ms = 20) => new Promise((r) => setTimeout(r, ms));

describe("request signing", () => {
  test("HMAC-SHA256, base64, matches an independently computed vector", () => {
    // printf '{"type":"isready","content":{}}' | openssl dgst -sha256 -hmac secret-key -binary | base64
    assert.equal(
      signPayload('{"type":"isready","content":{}}', "secret-key"),
      "vH5N8x/OtM+TpOPX6Xr+GkWnq4JvsSo6n2Jiw9gMVt4=",
    );
  });

  test("request line is {hmac,payload} JSON ending in one newline", () => {
    const payload = '{"type":"isready","content":{}}';
    const line = buildRequestLine(payload, "secret-key");
    assert.ok(line.endsWith("}\n"));
    assert.equal(line.indexOf("\n"), line.length - 1);
    assert.deepEqual(JSON.parse(line), {
      hmac: "vH5N8x/OtM+TpOPX6Xr+GkWnq4JvsSo6n2Jiw9gMVt4=",
      payload,
    });
  });

  test("a payload with non-ASCII text signs its UTF-8 bytes", () => {
    // printf '%s' '{"text":"héllo wörld ✓ 𝄞"}' | openssl dgst -sha256 -hmac k -binary | base64
    assert.equal(
      signPayload('{"text":"héllo wörld ✓ 𝄞"}', "k"),
      "IYAKJEWUlSDtIRwM6F/Rv1fbhMb4h/qspcaOg8Aokzg=",
    );
  });
});

describe("line framing", () => {
  test("a message split across chunks, including mid UTF-8 character", async () => {
    const message = JSON.stringify({ text: "añb✓𝄞c" }) + "\n";
    const bytes = Buffer.from(message, "utf8");
    const port = await fakeServer(async (s) => {
      // split at every byte, so every multi-byte character is cut
      for (const b of bytes) {
        s.write(Buffer.from([b]));
        await pause(1);
      }
    });
    const conn = await connectTo(port);
    assert.deepEqual(JSON.parse(await conn.readLine()), { text: "añb✓𝄞c" });
  });

  test("several messages in one chunk are delivered one at a time", async () => {
    const port = await fakeServer((s) => s.write('{"a":1}\n{"a":2}\n{"a":3}\n'));
    const conn = await connectTo(port);
    assert.equal(await conn.readLine(), '{"a":1}');
    assert.equal(await conn.readLine(), '{"a":2}');
    assert.equal(await conn.readLine(), '{"a":3}');
  });

  test("a chunk ending exactly on a newline leaves nothing behind", async () => {
    const port = await fakeServer(async (s) => {
      s.write('{"a":1}\n');
      await pause();
      s.write('{"a":2}\n');
    });
    const conn = await connectTo(port);
    assert.equal(await conn.readLine(), '{"a":1}');
    assert.equal(await conn.readLine(), '{"a":2}');
  });

  test("a chunk may end mid-message and the next complete it plus start another", async () => {
    const port = await fakeServer(async (s) => {
      s.write('{"a":1');
      await pause();
      s.write('}\n{"a":2');
      await pause();
      s.write("}\n");
    });
    const conn = await connectTo(port);
    assert.equal(await conn.readLine(), '{"a":1}');
    assert.equal(await conn.readLine(), '{"a":2}');
  });

  test("messages around and far above the old 512-byte buffer", async () => {
    const sizes = [511, 512, 513, 1024, 100_000, 5_000_000];
    const port = await fakeServer((s) => {
      for (const n of sizes) s.write("x".repeat(n) + "\n");
    });
    const conn = await connectTo(port);
    for (const n of sizes) {
      assert.equal((await conn.readLine()).length, n);
    }
  });

  test("a message read after a pause is still queued", async () => {
    const port = await fakeServer((s) => s.write("first\nsecond\n"));
    const conn = await connectTo(port);
    assert.equal(await conn.readLine(), "first");
    await pause();
    assert.equal(await conn.readLine(), "second");
  });
});

describe("failures", () => {
  test("close before any reply gives a clear error", async () => {
    const port = await fakeServer((s) => s.end());
    const conn = await connectTo(port);
    await assert.rejects(conn.readLine(), /closed before a reply/);
  });

  test("close mid-message gives a clear error, not a parse error", async () => {
    const port = await fakeServer((s) => s.end('{"half":'));
    const conn = await connectTo(port);
    await assert.rejects(conn.readLine(), /closed in the middle of a message/);
  });

  test("complete messages sent before the close are still delivered", async () => {
    const port = await fakeServer((s) => s.end('{"a":1}\n'));
    const conn = await connectTo(port);
    assert.equal(await conn.readLine(), '{"a":1}');
    await assert.rejects(conn.readLine(), /closed before a reply/);
  });

  test("connection reset surfaces the socket error", async () => {
    const port = await fakeServer((s) => s.resetAndDestroy());
    // The reset may land during connect (Linux/macOS) or on the first read
    // (Windows), so both steps sit inside the rejecting promise.
    await assert.rejects(
      async () => {
        const conn = await connectTo(port);
        await conn.readLine();
      },
      (e: NodeJS.ErrnoException) =>
        e.code === "ECONNRESET" || /closed/.test(e.message),
    );
  });

  test("connecting to a closed port rejects with ECONNREFUSED", async () => {
    const port = await fakeServer(() => {});
    await new Promise((r) => servers[0].close(r));
    await assert.rejects(
      LineConnection.connect(port),
      (e: NodeJS.ErrnoException) => e.code === "ECONNREFUSED",
    );
  });

  test("writing after the server went away rejects", async () => {
    const port = await fakeServer((s) => s.destroy());
    const conn = await connectTo(port);
    await assert.rejects(conn.readLine());
    await assert.rejects(conn.writeLine("x\n"));
  });
});

describe("write backpressure", () => {
  test("a large write completes once the peer drains it", async () => {
    let received = 0;
    let done: () => void;
    const all = new Promise<void>((r) => (done = r));
    const total = 20_000_000;
    const port = await fakeServer((s) => {
      s.on("data", (d) => {
        received += d.length;
        if (received >= total) done();
      });
    });
    const conn = await connectTo(port);
    await conn.writeLine("y".repeat(total - 1) + "\n");
    await all;
    assert.equal(received, total);
  });
});

describe("sendCommand", () => {
  // Echo server speaking the control protocol: replies to each request line
  // using `respond(request, socket)`.
  async function protocolServer(
    respond: (req: { hmac: string; payload: string }, s: Socket) => void,
  ) {
    return await fakeServer((s) => {
      let buf = "";
      s.on("data", (d) => {
        buf += d.toString("utf8");
        let nl;
        while ((nl = buf.indexOf("\n")) !== -1) {
          respond(JSON.parse(buf.slice(0, nl)), s);
          buf = buf.slice(nl + 1);
        }
      });
    });
  }

  test("sends a signed request and returns the response", async () => {
    let seen: { hmac: string; payload: string } | undefined;
    const port = await protocolServer((req, s) => {
      seen = req;
      s.write("true\n");
    });
    const conn = await connectTo(port);
    const result = await sendCommand(
      conn,
      { type: "isready", content: {} } as { type: string },
      "secret-key",
    );
    assert.equal(result, true);
    assert.equal(seen!.payload, '{"type":"isready","content":{}}');
    assert.equal(seen!.hmac, signPayload(seen!.payload, "secret-key"));
  });

  test("run streams progress updates before the notebook", async () => {
    const port = await protocolServer((_req, s) => {
      const update = (i: number) =>
        JSON.stringify({
          type: "progress_update",
          chunkIndex: i,
          nChunks: 2,
          source: "1+1",
          line: i * 3,
        });
      // updates and the final response arrive glued together in one chunk
      s.write(update(1) + "\n" + update(2) + "\n" + '{"notebook":{"cells":[]}}\n');
    });
    const conn = await connectTo(port);
    const updates: ProgressUpdate[] = [];
    const result = await sendCommand(
      conn,
      { type: "run", content: {} } as { type: string },
      "k",
      undefined,
      (u) => updates.push(u),
    );
    assert.deepEqual(updates.map((u) => u.chunkIndex), [1, 2]);
    assert.deepEqual(result, { notebook: { cells: [] } });
  });

  test("progress_update is only special for run commands", async () => {
    const port = await protocolServer((_req, s) =>
      s.write('{"type":"progress_update"}\n')
    );
    const conn = await connectTo(port);
    const result = await sendCommand(conn, { type: "status" }, "k");
    assert.deepEqual(result, { type: "progress_update" });
  });

  test("an error response throws with the julia error appended", async () => {
    const port = await protocolServer((_req, s) =>
      s.write('{"error":"worker is busy","juliaError":"BoundsError"}\n')
    );
    const conn = await connectTo(port);
    await assert.rejects(
      sendCommand(conn, { type: "close" }, "k"),
      (e: Error) =>
        e.message.includes('after receiving "close" command') &&
        e.message.includes("worker is busy") &&
        e.message.includes("The underlying Julia error was:\n\nBoundsError"),
    );
  });

  test("an error response without juliaError omits that section", async () => {
    const port = await protocolServer((_req, s) => s.write('{"error":"nope"}\n'));
    const conn = await connectTo(port);
    await assert.rejects(
      sendCommand(conn, { type: "close" }, "k"),
      (e: Error) => !e.message.includes("underlying Julia error"),
    );
  });

  test("an unparseable response is reported as such", async () => {
    const port = await protocolServer((_req, s) => s.write("not json\n"));
    const conn = await connectTo(port);
    await assert.rejects(
      sendCommand(conn, { type: "status" }, "k"),
      /unparseable response to the "status" command/,
    );
  });

  test("the server closing mid-run rejects the command", async () => {
    const port = await protocolServer((_req, s) => {
      s.write('{"type":"progress_update","chunkIndex":1,"nChunks":3,"source":"x","line":1}\n');
      s.end('{"notebo');
    });
    const conn = await connectTo(port);
    await assert.rejects(
      sendCommand(conn, { type: "run" }, "k", undefined, () => {}),
      /closed in the middle of a message/,
    );
  });

  test("leftover bytes after a response stay available for the next command", async () => {
    let n = 0;
    const port = await protocolServer((_req, s) => {
      n++;
      // the first reply carries the second reply too
      if (n === 1) s.write('{"n":1}\n{"n":2}\n');
    });
    const conn = await connectTo(port);
    assert.deepEqual(await sendCommand(conn, { type: "status" }, "k"), { n: 1 });
    // the second request's reply is already buffered
    assert.deepEqual(await sendCommand(conn, { type: "status" }, "k"), { n: 2 });
  });
});

describe("connectReady", () => {
  test("returns the connection once the server answers true", async () => {
    const port = await fakeServer((s) => s.on("data", () => s.write("true\n")));
    const conn = await connectReady(port, "k", 1000);
    assert.ok(conn instanceof LineConnection);
    conns.push(conn as LineConnection);
  });

  test("a silent server times out and the socket is destroyed", async () => {
    let serverSawClose: () => void;
    const closed = new Promise<void>((r) => (serverSawClose = r));
    const port = await fakeServer((s) => {
      s.resume(); // read (and discard) so the hang-up is noticed
      s.on("close", () => serverSawClose());
    });
    const start = Date.now();
    const result = await connectReady(port, "k", 100);
    assert.match(result as string, /Timed out after getting no response for 100 milliseconds/);
    assert.ok(Date.now() - start < 2000);
    await closed; // the client hung up instead of leaking the socket
  });

  test("a server that answers something other than true is rejected", async () => {
    const port = await fakeServer((s) => s.on("data", () => s.write("false\n")));
    const result = await connectReady(port, "k", 1000);
    assert.match(result as string, /Expected isready command to return true, returned false/);
  });

  test("a server error reply rejects", async () => {
    const port = await fakeServer((s) =>
      s.on("data", () => s.write('{"error":"bad hmac"}\n'))
    );
    await assert.rejects(connectReady(port, "k", 1000), /bad hmac/);
  });

  test("a refused connection rejects with ECONNREFUSED", async () => {
    const port = await fakeServer(() => {});
    await new Promise((r) => servers[0].close(r));
    await assert.rejects(
      connectReady(port, "k", 1000),
      (e: NodeJS.ErrnoException) => e.code === "ECONNREFUSED",
    );
  });
});
