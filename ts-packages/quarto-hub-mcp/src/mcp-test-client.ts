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

  /**
   * Start the MCP server process with the given arguments.
   */
  async start(args: string[]): Promise<void> {
    this.proc = spawn('node', [SERVER_ENTRY, ...args], {
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    this.proc.stdout!.setEncoding('utf-8');
    this.proc.stdout!.on('data', (chunk: string) => {
      this.buffer += chunk;
      this.parseResponses();
    });

    this.proc.stderr!.setEncoding('utf-8');
    this.proc.stderr!.on('data', (chunk: string) => {
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
