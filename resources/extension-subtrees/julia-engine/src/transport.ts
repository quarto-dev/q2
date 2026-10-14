/*
 * transport.ts
 *
 * Wire protocol to the QuartoNotebookRunner control server: HMAC-signed JSON
 * requests, newline-delimited JSON responses over a local TCP socket.
 * Independent of the Quarto API so it can be tested against a fake server.
 */

import { createHmac } from "node:crypto";
import { connect, type Socket } from "node:net";

/** base64(HMAC-SHA256(secret, payload)), the signature the server verifies. */
export function signPayload(payload: string, secret: string): string {
  return createHmac("sha256", secret).update(payload).digest("base64");
}

/** The signed, newline-terminated request line for a JSON payload. */
export function buildRequestLine(payload: string, secret: string): string {
  return JSON.stringify({ hmac: signPayload(payload, secret), payload }) +
    "\n";
}

const NEWLINE = 10;

/**
 * A TCP connection that reads and writes newline-delimited messages.
 *
 * Incoming bytes are buffered until a newline, and only a complete line is
 * decoded, so a multi-byte UTF-8 character split across chunks is intact.
 * Complete lines queue up, so several messages arriving in one chunk are
 * delivered one `readLine()` at a time.
 */
export class LineConnection {
  private parts: Buffer[] = [];
  private lines: string[] = [];
  private waiter: { resolve: (line: string) => void; reject: (e: Error) => void } | null = null;
  private failure: Error | null = null;

  private socket: Socket;

  private constructor(socket: Socket) {
    this.socket = socket;
    socket.on("data", (chunk: Buffer) => this.onData(chunk));
    socket.on("error", (e) => this.fail(e));
    socket.on("close", () => {
      this.fail(
        new Error(
          this.parts.length > 0
            ? "Connection to the julia server closed in the middle of a message."
            : "Connection to the julia server closed before a reply was received.",
        ),
      );
    });
  }

  static connect(port: number, host = "127.0.0.1"): Promise<LineConnection> {
    return new Promise((resolve, reject) => {
      const socket = connect({ host, port });
      const onError = (e: Error) => reject(e);
      socket.once("error", onError);
      socket.once("connect", () => {
        socket.off("error", onError);
        resolve(new LineConnection(socket));
      });
    });
  }

  private onData(chunk: Buffer) {
    let start = 0;
    while (true) {
      const nl = chunk.indexOf(NEWLINE, start);
      if (nl === -1) {
        if (start < chunk.length) this.parts.push(chunk.subarray(start));
        return;
      }
      this.parts.push(chunk.subarray(start, nl));
      this.lines.push(Buffer.concat(this.parts).toString("utf8"));
      this.parts = [];
      start = nl + 1;
      this.flush();
    }
  }

  private flush() {
    if (this.waiter && this.lines.length > 0) {
      const { resolve } = this.waiter;
      this.waiter = null;
      resolve(this.lines.shift()!);
    } else if (this.waiter && this.failure && this.lines.length === 0) {
      const { reject } = this.waiter;
      this.waiter = null;
      reject(this.failure);
    }
  }

  private fail(e: Error) {
    // the first failure is the cause, the close event that follows is not
    this.failure ??= e;
    this.flush();
  }

  /** Write one message (the caller supplies the trailing newline). */
  writeLine(line: string): Promise<void> {
    return new Promise((resolve, reject) => {
      if (this.failure) return reject(this.failure);
      // the callback fires once the data is flushed, so awaiting it
      // honours backpressure
      this.socket.write(line, "utf8", (e) => e ? reject(e) : resolve());
    });
  }

  /** The next complete line, without its newline. */
  readLine(): Promise<string> {
    if (this.lines.length > 0) return Promise.resolve(this.lines.shift()!);
    if (this.failure) return Promise.reject(this.failure);
    return new Promise((resolve, reject) => {
      this.waiter = { resolve, reject };
    });
  }

  close() {
    this.socket.destroy();
  }
}

export interface ProgressUpdate {
  type: "progress_update";
  chunkIndex: number;
  nChunks: number;
  source: string;
  line: number;
}

type ServerCommandError = {
  error: string;
  juliaError?: string;
};

function isProgressUpdate(data: any): data is ProgressUpdate {
  return data && data.type === "progress_update";
}

function isServerCommandError(data: any): data is ServerCommandError {
  return data && typeof (data.error) === "string";
}

/**
 * Send one command and return the server's response. For `run` commands the
 * server first streams progress updates, which go to `onProgressUpdate`.
 */
export async function sendCommand(
  conn: LineConnection,
  command: { type: string },
  secret: string,
  trace: (msg: string) => void = () => {},
  onProgressUpdate?: (update: ProgressUpdate) => void,
): Promise<any> {
  trace(`write command "${command.type}" to socket server`);
  await conn.writeLine(buildRequestLine(JSON.stringify(command), secret));

  while (true) {
    const line = await conn.readLine();
    trace("received server response");

    let responseData: any;
    try {
      responseData = JSON.parse(line);
    } catch (e) {
      throw new Error(
        `Julia server sent an unparseable response to the "${command.type}" command: ${e}`,
      );
    }

    if (isServerCommandError(responseData)) {
      let errorMessage =
        `Julia server returned error after receiving "${command.type}" command:\n\n${responseData.error}`;
      if (responseData.juliaError) {
        errorMessage +=
          `\n\nThe underlying Julia error was:\n\n${responseData.juliaError}`;
      }
      throw new Error(errorMessage);
    }

    if (command.type === "run" && isProgressUpdate(responseData)) {
      trace("received progress update response, listening for further responses");
      onProgressUpdate?.(responseData);
      continue; // wait for the next message
    }
    return responseData;
  }
}

/**
 * Connect and check the server answers `isready` within `timeoutMs`. Returns
 * the ready connection, or a string describing why it is unusable (the
 * connection is closed in that case). Rejects if the connection can't be made.
 */
export async function connectReady(
  port: number,
  secret: string,
  timeoutMs = 10000,
  trace: (msg: string) => void = () => {},
): Promise<LineConnection | string> {
  const conn = await LineConnection.connect(port);
  const isready = sendCommand(
    conn,
    { type: "isready", content: {} } as { type: string },
    secret,
    trace,
  );
  let timer: ReturnType<typeof setTimeout>;
  const timeout = new Promise<string>((accept) => {
    timer = setTimeout(() => {
      accept(
        `Timed out after getting no response for ${timeoutMs} milliseconds.`,
      );
    }, timeoutMs);
  });
  let result;
  try {
    result = await Promise.race([isready, timeout]);
  } catch (e) {
    conn.close();
    throw e;
  } finally {
    clearTimeout(timer!);
  }
  if (typeof result === "string") {
    // the connection is useless now; closing it also settles the pending
    // isready request, whose rejection nobody is waiting for
    conn.close();
    isready.catch(() => {});
    return result;
  } else if (result !== true) {
    conn.close();
    return `Expected isready command to return true, returned ${result} instead. Closing connection.`;
  } else {
    return conn;
  }
}
