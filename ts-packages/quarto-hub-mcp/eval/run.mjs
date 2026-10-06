#!/usr/bin/env node
/**
 * Agent-task eval runner (ERG-7, Phase 0, bd-f1dr7gs1;
 * claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md).
 *
 * Runs the scripted tasks in tasks.mjs against the in-process test-hub
 * with headless `claude -p` as the agent host (Q-7: chosen over the
 * Agent SDK — `--output-format stream-json` already yields
 * machine-readable transcripts with zero new dependencies, and it
 * exercises the Claude Code host agents actually use). Each task spawns
 * a fresh Claude session whose only tool surface is the quarto-hub MCP
 * server (built from dist/), mirroring the §9 "no shell access" shape.
 *
 * NOT in CI: cost and nondeterminism. Run at phase gates by hand:
 *
 *   npm run eval -w ts-packages/quarto-hub-mcp                 # all tasks
 *   node eval/run.mjs --only patch-typo                        # one task
 *
 * Scoring per task (recorded in eval/results/<timestamp>/summary.json):
 * success (hub-state check), turns, tokens, cost, isError tool results,
 * validation retries (a tool call whose previous result for the same
 * tool was isError — heuristic), denied non-MCP tool attempts.
 */

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

import { startTestHub } from '../dist/test-hub.js';
import { ConnectionManager } from '../dist/connection-manager.js';
import { TASKS } from './tasks.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = path.resolve(__dirname, '..');
const SERVER_ENTRY = path.join(PKG_ROOT, 'dist', 'index.js');
const RESULTS_ROOT = path.join(__dirname, 'results');

const MAX_TURNS = 30;
const TASK_HARD_TIMEOUT_MS = 6 * 60 * 1000;

// ---------------------------------------------------------------------------
// Preflight
// ---------------------------------------------------------------------------

if (!existsSync(SERVER_ENTRY)) {
  console.error(`dist/index.js not found — run \`npm run build -w ts-packages/quarto-hub-mcp\` first.`);
  process.exit(1);
}
const claudeVersion = spawnSync('claude', ['--version'], { encoding: 'utf-8' });
if (claudeVersion.status !== 0) {
  console.error('`claude` CLI not found on PATH (or errored) — the eval suite needs Claude Code.');
  process.exit(1);
}

const onlyIdx = process.argv.indexOf('--only');
const only = onlyIdx !== -1 ? process.argv[onlyIdx + 1] : undefined;
const tasks = only ? TASKS.filter((t) => t.id === only) : TASKS;
if (tasks.length === 0) {
  console.error(`no task matches --only ${only}; known: ${TASKS.map((t) => t.id).join(', ')}`);
  process.exit(1);
}

// ---------------------------------------------------------------------------
// Hub-side helpers (seed / observe via the runner's own connection)
// ---------------------------------------------------------------------------

const hub = await startTestHub();
const manager = new ConnectionManager({ serverUrl: hub.url });

const ctx = {
  hub,
  manager,
  async seedProject(files) {
    const { indexDocId, files: created } = await manager.createProject(files);
    if (!(await hub.hubHasDoc(indexDocId, 10000))) {
      throw new Error('seed: hub never received the index document');
    }
    for (const f of created) {
      if (!(await hub.hubHasDoc(f.docId, 10000))) {
        throw new Error(`seed: hub never received ${f.path}`);
      }
    }
    return { indexDocId };
  },
  async readFile(indexDocId, p) {
    // The project may have been created by the agent's server moments
    // ago; its exit drain can still be in flight when the check runs.
    // Poll instead of racing one-shot connect.
    const deadline = Date.now() + 15000;
    for (;;) {
      try {
        const state = await manager.connect(indexDocId);
        const payload = state.files.get(p);
        if (payload !== undefined) {
          return payload.type === 'text' ? payload.text : undefined;
        }
      } catch {
        // index doc not at the hub yet — keep polling until the deadline
      }
      if (Date.now() > deadline) return undefined;
      await new Promise((r) => setTimeout(r, 250));
    }
  },
  async listPaths(indexDocId) {
    const deadline = Date.now() + 15000;
    for (;;) {
      try {
        const state = await manager.connect(indexDocId);
        if (state.files.size > 0) return [...state.files.keys()].sort();
      } catch {
        // not delivered yet
      }
      if (Date.now() > deadline) return [];
      await new Promise((r) => setTimeout(r, 250));
    }
  },
  async editFile(indexDocId, p, text) {
    const state = await manager.connect(indexDocId);
    state.client.updateFileContent(p, text);
  },
};

// ---------------------------------------------------------------------------
// One task run
// ---------------------------------------------------------------------------

function parseStreamEvents(raw) {
  const events = [];
  for (const line of raw.split('\n')) {
    const t = line.trim();
    if (!t) continue;
    try {
      events.push(JSON.parse(t));
    } catch {
      // claude may print non-JSON diagnostics; ignore for scoring
    }
  }
  return events;
}

function scoreEvents(events) {
  const toolNames = new Map(); // tool_use_id → name
  let toolCalls = 0;
  let nonMcpToolUses = 0;
  let isErrorCount = 0;
  let validationRetries = 0;
  let model;
  const lastErrored = new Map(); // tool name → true when its last result was is_error

  for (const ev of events) {
    if (ev.type === 'system' && ev.subtype === 'init' && ev.model) model = ev.model;
    if (ev.type === 'assistant') {
      for (const block of ev.message?.content ?? []) {
        if (block.type !== 'tool_use') continue;
        const name = String(block.name);
        toolNames.set(block.id, name);
        if (name.startsWith('mcp__quarto-hub__')) {
          toolCalls++;
          if (lastErrored.get(name)) validationRetries++;
          lastErrored.set(name, false);
        } else {
          // Host-side tools (e.g. Claude Code's ToolSearch discovery) —
          // informative when the agent reaches outside the MCP surface.
          nonMcpToolUses++;
        }
      }
    } else if (ev.type === 'user') {
      for (const block of ev.message?.content ?? []) {
        if (block.type !== 'tool_result') continue;
        const name = toolNames.get(block.tool_use_id);
        if (name?.startsWith('mcp__quarto-hub__') && block.is_error === true) {
          isErrorCount++;
          lastErrored.set(name, true);
        }
      }
    }
  }
  return { toolCalls, nonMcpToolUses, isErrorCount, validationRetries, model };
}

function condenseTranscript(events) {
  const lines = [];
  for (const ev of events) {
    const clip = (s, n = 300) => {
      const t = typeof s === 'string' ? s : JSON.stringify(s);
      return t.length > n ? t.slice(0, n) + '…' : t;
    };
    if (ev.type === 'system' && ev.subtype === 'init') {
      lines.push(`# init: model=${ev.model ?? '?'} tools=${(ev.tools ?? []).length}`);
    } else if (ev.type === 'assistant') {
      for (const b of ev.message?.content ?? []) {
        if (b.type === 'text') lines.push(`\n**assistant:** ${clip(b.text, 500)}`);
        if (b.type === 'tool_use') lines.push(`\n**tool_use ${b.name}:** ${clip(b.input)}`);
      }
    } else if (ev.type === 'user') {
      for (const b of ev.message?.content ?? []) {
        if (b.type === 'tool_result') {
          lines.push(`**tool_result${b.is_error ? ' (ERROR)' : ''}:** ${clip(b.content)}`);
        }
      }
    } else if (ev.type === 'result') {
      lines.push(
        `\n# result: subtype=${ev.subtype} is_error=${ev.is_error} turns=${ev.num_turns} ` +
          `cost=$${ev.total_cost_usd?.toFixed(4)}\n\n${clip(ev.result, 1000)}`,
      );
    }
  }
  return lines.join('\n');
}

async function runTask(task, resultsDir) {
  await task.setup(ctx);

  const configDir = await mkdtemp(path.join(tmpdir(), 'hub-mcp-eval-'));
  const configPath = path.join(configDir, 'mcp.json');
  writeFileSync(
    configPath,
    JSON.stringify({
      mcpServers: {
        'quarto-hub': {
          command: process.execPath,
          args: [SERVER_ENTRY, '--server', hub.url],
        },
      },
    }),
  );

  const env = { ...process.env };
  delete env['QUARTO_HUB_MCP_CLIENT_ID'];
  delete env['QUARTO_HUB_MCP_CLIENT_SECRET'];

  const startedAt = Date.now();
  const child = spawn(
    'claude',
    [
      '-p',
      task.prompt(ctx),
      '--mcp-config',
      configPath,
      '--strict-mcp-config',
      '--output-format',
      'stream-json',
      '--verbose',
      '--max-turns',
      String(MAX_TURNS),
      '--allowedTools',
      'mcp__quarto-hub',
    ],
    { env, stdio: ['ignore', 'pipe', 'pipe'] },
  );

  let stdout = '';
  let stderr = '';
  child.stdout.setEncoding('utf-8').on('data', (d) => (stdout += d));
  child.stderr.setEncoding('utf-8').on('data', (d) => (stderr += d));

  const ticker = setInterval(() => {
    if (task.onTick) void task.onTick(ctx, Date.now() - startedAt);
  }, 1000);
  const killer = setTimeout(() => child.kill('SIGKILL'), TASK_HARD_TIMEOUT_MS);
  const code = await new Promise((resolve) => child.on('exit', resolve));
  clearInterval(ticker);
  clearTimeout(killer);
  const durationMs = Date.now() - startedAt;

  const events = parseStreamEvents(stdout);
  const resultEvent = events.find((e) => e.type === 'result');
  const finalText = typeof resultEvent?.result === 'string' ? resultEvent.result : '';
  const scoring = scoreEvents(events);

  const checkPass = await task.check(ctx, finalText).catch(() => false);
  const agentError = resultEvent?.is_error !== false; // missing result event ⇒ error
  const success = checkPass && !agentError;

  writeFileSync(path.join(resultsDir, 'raw', `${task.id}.jsonl`), stdout);
  writeFileSync(
    path.join(resultsDir, `transcript-${task.id}.md`),
    `# ${task.id}\n\nprompt: ${task.prompt(ctx)}\n\n${condenseTranscript(events)}\n` +
      (stderr.trim() ? `\n## stderr (claude)\n\n${stderr.slice(0, 2000)}\n` : ''),
  );

  return {
    task: task.id,
    success,
    checkPass,
    agentError,
    exitCode: code,
    turns: resultEvent?.num_turns ?? null,
    inputTokens: resultEvent?.usage?.input_tokens ?? null,
    outputTokens: resultEvent?.usage?.output_tokens ?? null,
    costUsd: resultEvent?.total_cost_usd ?? null,
    durationMs,
    ...scoring,
  };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
const resultsDir = path.join(RESULTS_ROOT, stamp);
mkdirSync(path.join(resultsDir, 'raw'), { recursive: true });

console.error(`eval run ${stamp} — ${tasks.length} task(s) against ${hub.url}`);
const summaries = [];
for (const task of tasks) {
  console.error(`  → ${task.id} …`);
  const s = await runTask(task, resultsDir);
  summaries.push(s);
  console.error(
    `    ${s.success ? 'PASS' : 'FAIL'} turns=${s.turns} isError=${s.isErrorCount} ` +
      `retries=${s.validationRetries} nonMcp=${s.nonMcpToolUses} ` +
      `${(s.durationMs / 1000).toFixed(0)}s $${s.costUsd?.toFixed(3) ?? '?'}`,
  );
}

const gitRev = spawnSync('git', ['rev-parse', '--short', 'HEAD'], { encoding: 'utf-8' }).stdout.trim();
writeFileSync(
  path.join(resultsDir, 'summary.json'),
  JSON.stringify(
    {
      date: new Date().toISOString(),
      claudeVersion: claudeVersion.stdout.trim(),
      gitRev,
      tasks: summaries,
    },
    null,
    2,
  ),
);

console.log('\n| task | success | turns | tokens (in/out) | cost | isError | retries | nonMcp | duration |');
console.log('|------|---------|-------|-----------------|------|---------|---------|--------|----------|');
for (const s of summaries) {
  console.log(
    `| ${s.task} | ${s.success ? 'PASS' : 'FAIL'} | ${s.turns} | ${s.inputTokens}/${s.outputTokens} ` +
      `| $${s.costUsd?.toFixed(4) ?? '?'} | ${s.isErrorCount} | ${s.validationRetries} | ` +
      `${s.nonMcpToolUses} | ${(s.durationMs / 1000).toFixed(0)}s |`,
  );
}
console.log(`\nresults: ${resultsDir}`);

await manager.disconnectAll({ drainMs: 0 });
await hub.stop();
// Trend instrument, not a pass/fail gate (plan §8): exit 0 regardless.
process.exit(0);
