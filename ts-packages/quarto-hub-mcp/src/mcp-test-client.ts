/**
 * Test helper: spawns the MCP server as a child process and communicates
 * via JSON-RPC over stdio.
 */

import { spawn, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const SERVER_ENTRY = path.resolve(__dirname, '../dist/index.js');

interface JsonRpcResponse {
  jsonrpc: '2.0';
  id: number;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

export class McpTestClient {
  private proc: ChildProcess | null = null;
  private buffer = '';
  private responses: JsonRpcResponse[] = [];
  private waiters: Array<() => void> = [];
  private nextId = 1;

  /**
   * Every stdout line that failed to parse as JSON-RPC. In a correct
   * stdio MCP server this stays empty: stdout belongs exclusively to
   * the protocol, and any stray write here corrupts the stream for
   * real clients.
   */
  readonly stdoutPollution: string[] = [];

  /** Server stderr, line by line (diagnostics; the auth e2e scrapes
   * the sign-in URL from here). */
  readonly stderrLines: string[] = [];
  private stderrBuffer = '';

  /**
   * Start the MCP server process with the given arguments.
   *
   * `entry` overrides the server entry point (default: the tsc build
   * at dist/index.js) — used by bundle tests to drive the esbuild
   * artifact from outside the repo tree. `command` (+ leading args)
   * replaces `node` entirely — used by the auth e2e to drive the real
   * `q2 mcp` launcher. `env` REPLACES the child environment when given
   * (callers spread process.env themselves if they want it); when omitted
   * the child inherits process.env minus the auth-gating vars
   * (QUARTO_HUB_MCP_CLIENT_ID/SECRET) so the server runs unauthenticated
   * regardless of the developer's shell (bd-uyiqciqk).
   */
  async start(
    args: string[],
    opts?: { entry?: string; command?: { program: string; args: string[] }; env?: NodeJS.ProcessEnv },
  ): Promise<void> {
    const [program, leading] = opts?.command
      ? [opts.command.program, opts.command.args]
      : ['node', [opts?.entry ?? SERVER_ENTRY]];
    // Don't let the developer's real auth config leak into the spawned
    // server: with QUARTO_HUB_MCP_CLIENT_ID/SECRET set it takes the OS-keychain
    // credential path, which fails nondeterministically against a populated
    // keychain. Strip them from the inherited env unless a test passes its own.
    let env = opts?.env;
    if (!env) {
      env = { ...process.env };
      delete env['QUARTO_HUB_MCP_CLIENT_ID'];
      delete env['QUARTO_HUB_MCP_CLIENT_SECRET'];
    }
    this.proc = spawn(program, [...leading, ...args], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env,
    });

    this.proc.stdout!.setEncoding('utf-8');
    this.proc.stdout!.on('data', (chunk: string) => {
      this.buffer += chunk;
      this.parseResponses();
    });

    this.proc.stderr!.setEncoding('utf-8');
    this.proc.stderr!.on('data', (chunk: string) => {
      this.stderrBuffer += chunk;
      let nl;
      while ((nl = this.stderrBuffer.indexOf('\n')) >= 0) {
        this.stderrLines.push(this.stderrBuffer.slice(0, nl));
        this.stderrBuffer = this.stderrBuffer.slice(nl + 1);
      }
      // Suppress stderr noise in tests unless debugging
      if (process.env['DEBUG_MCP']) {
        process.stderr.write(`[mcp-stderr] ${chunk}`);
      }
    });

    // Initialize the MCP session
    await this.sendRequest('initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'test-client', version: '1.0' },
    });

    // Send initialized notification
    this.sendNotification('notifications/initialized');
  }

  /**
   * Wait until a stderr line matching `pattern` appears (scanning
   * lines already received too) and return the first match.
   */
  async waitForStderr(pattern: RegExp, timeoutMs = 10000): Promise<string> {
    const deadline = Date.now() + timeoutMs;
    let scanned = 0;
    while (Date.now() < deadline) {
      for (; scanned < this.stderrLines.length; scanned++) {
        const line = this.stderrLines[scanned]!;
        if (pattern.test(line)) return line;
      }
      await new Promise((r) => setTimeout(r, 25));
    }
    throw new Error(
      `timed out waiting for stderr matching ${pattern}; saw:\n${this.stderrLines.join('\n')}`,
    );
  }

  /**
   * Close the server's stdin (how MCP hosts terminate stdio servers)
   * and report whether the process exited within `timeoutMs`. Unlike
   * `stop()`, does NOT kill the process on timeout — callers assert on
   * the result and then call `stop()` to clean up.
   */
  async endStdinAndWaitForExit(timeoutMs = 5000): Promise<boolean> {
    if (!this.proc) throw new Error('server not started');
    if (this.proc.exitCode !== null) return true;
    const exited = once(this.proc, 'exit').then(() => true);
    this.proc.stdin!.end();
    const timedOut = new Promise<boolean>((resolve) =>
      setTimeout(() => resolve(false), timeoutMs),
    );
    return Promise.race([exited, timedOut]);
  }

  /**
   * Stop the MCP server process.
   */
  async stop(): Promise<void> {
    if (!this.proc) return;
    this.proc.stdin!.end();
    // Wait for process to exit, with timeout
    const exitPromise = once(this.proc, 'exit');
    const timeout = new Promise<void>((resolve) => setTimeout(resolve, 3000));
    await Promise.race([exitPromise, timeout]);
    if (this.proc.exitCode === null) {
      this.proc.kill('SIGKILL');
    }
    this.proc = null;
  }

  /**
   * Send a JSON-RPC request and wait for the response.
   */
  async sendRequest(method: string, params?: unknown): Promise<JsonRpcResponse> {
    const id = this.nextId++;
    const message = JSON.stringify({
      jsonrpc: '2.0',
      id,
      method,
      params: params ?? {},
    });
    this.proc!.stdin!.write(message + '\n');

    // Wait for the response with this ID
    return this.waitForResponse(id);
  }

  /**
   * Send a JSON-RPC notification (no response expected).
   */
  sendNotification(method: string, params?: unknown): void {
    const message = JSON.stringify({
      jsonrpc: '2.0',
      method,
      params: params ?? {},
    });
    this.proc!.stdin!.write(message + '\n');
  }

  /**
   * Call an MCP tool and return the result.
   */
  async callTool(name: string, args: Record<string, unknown>): Promise<{
    content: Array<{ type: string; text: string }>;
    isError?: boolean;
  }> {
    const response = await this.sendRequest('tools/call', {
      name,
      arguments: args,
    });
    if (response.error) {
      throw new Error(`MCP error: ${response.error.message}`);
    }
    return response.result as {
      content: Array<{ type: string; text: string }>;
      isError?: boolean;
    };
  }

  /**
   * List all available tools.
   */
  async listTools(): Promise<Array<{
    name: string;
    description: string;
    inputSchema: unknown;
    annotations?: unknown;
  }>> {
    const response = await this.sendRequest('tools/list');
    if (response.error) {
      throw new Error(`MCP error: ${response.error.message}`);
    }
    const result = response.result as { tools: Array<{
      name: string;
      description: string;
      inputSchema: unknown;
      annotations?: unknown;
    }> };
    return result.tools;
  }

  /**
   * List all available prompts.
   */
  async listPrompts(): Promise<Array<{ name: string; description?: string; arguments?: unknown }>> {
    const response = await this.sendRequest('prompts/list');
    if (response.error) {
      throw new Error(`MCP error: ${response.error.message}`);
    }
    return (response.result as { prompts: Array<{ name: string }> }).prompts;
  }

  /**
   * Get a prompt with arguments filled in.
   */
  async getPrompt(
    name: string,
    args: Record<string, string>,
  ): Promise<{ messages: Array<{ role: string; content: { type: string; text: string } }> }> {
    const response = await this.sendRequest('prompts/get', { name, arguments: args });
    if (response.error) {
      throw new Error(`MCP error: ${response.error.message}`);
    }
    return response.result as {
      messages: Array<{ role: string; content: { type: string; text: string } }>;
    };
  }

  // ---- Internal ----

  private parseResponses(): void {
    // MCP uses newline-delimited JSON
    const lines = this.buffer.split('\n');
    this.buffer = lines.pop()!; // Keep incomplete last line
    for (const line of lines) {
      if (line.trim()) {
        try {
          const parsed = JSON.parse(line) as JsonRpcResponse;
          if ('id' in parsed) {
            this.responses.push(parsed);
            // Wake any waiters
            const waiters = this.waiters;
            this.waiters = [];
            for (const w of waiters) w();
          }
        } catch {
          // Not JSON-RPC: record as protocol pollution (see field doc).
          this.stdoutPollution.push(line);
        }
      }
    }
  }

  private async waitForResponse(id: number, timeoutMs = 30000): Promise<JsonRpcResponse> {
    const deadline = Date.now() + timeoutMs;
    while (true) {
      const idx = this.responses.findIndex(r => r.id === id);
      if (idx !== -1) {
        return this.responses.splice(idx, 1)[0]!;
      }
      if (Date.now() > deadline) {
        throw new Error(`Timeout waiting for response to request ${id}`);
      }
      await new Promise<void>((resolve) => {
        const timer = setTimeout(resolve, timeoutMs);
        this.waiters.push(() => {
          clearTimeout(timer);
          resolve();
        });
      });
    }
  }
}
