# Agent-task eval suite (ERG-7)

Measures how many turns/tokens an agent needs for realistic Quarto Hub
tasks through this MCP server. Not in CI (cost, nondeterminism) — run at
phase gates by hand and record the summary in the plan
(`claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md`).

## Running

```bash
npm run eval -w ts-packages/quarto-hub-mcp      # build + all tasks
node eval/run.mjs --only patch-typo             # a single task (dist must be built)
```

Requires the `claude` CLI (Claude Code) on PATH, signed in. Each task
spawns a fresh `claude -p` session whose only tool surface is this MCP
server (built from `dist/`) connected to an in-process test-hub — no
network, no shell access for the agent (`--allowedTools mcp__quarto-hub`).

## Output

`eval/results/<timestamp>/`:

- `summary.json` — per-task scores: success (hub-state check), turns,
  tokens, cost, `isError` tool-result count, validation retries (a tool
  call whose previous result for the same tool was `isError` —
  heuristic), non-MCP tool uses (e.g. the host's ToolSearch discovery;
  a high count can signal the agent reaching outside the MCP surface).
- `transcript-<task>.md` — condensed, reviewable transcript (committed).
- `raw/<task>.jsonl` — full stream-json (gitignored; keep locally for
  digging into a regression).

## Scoring philosophy

Trend instrument, not a pass/fail gate (plan §8): the runner exits 0
regardless. Compare summaries across phases — the Phase 0 baseline is
recorded in the plan, and later phases must not regress median turns
while driving `isError`-from-argument-shape toward zero (§9).

Tasks are scored on **hub state and the final answer**, never on the
agent's exact tool sequence. New tools added in later phases should be
reflected by new tasks (or relaxed prompts), not by asserting which
tool the agent "should" have picked.
