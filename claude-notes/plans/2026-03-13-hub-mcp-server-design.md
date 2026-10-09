---
title: 'Hub MCP Server Design: Automerge Project Access for AI Agents'
date: 2026-03-13
---

## Overview

Design and implement an MCP server that provides AI coding agents (Claude Code, Codex, etc.) with direct access to Quarto Hub projects backed by automerge sync servers. Instead of requiring filesystem access, agents interact with project files through MCP tools that read/write automerge documents directly.

**Key insight**: `quarto-sync-client` already provides the complete API we need (connect, read, write, create, delete, rename files). The MCP server is a thin wrapper that exposes these operations as MCP tools.

## Context

### How Quarto Projects Are Represented in Automerge

- **Index Document**: `{ files: Record<string, string> }` — maps file paths to automerge document IDs
- **Text File Documents**: `{ text: string }` — uses Automerge Text type for incremental sync
- **Binary File Documents**: `{ content: Uint8Array, mimeType: string, hash: string }`
- Connection requires: **sync server URL** + **index document ID**

### Existing Infrastructure

- `ts-packages/quarto-sync-client` — TypeScript package with full file CRUD API over automerge
- `ts-packages/quarto-automerge-schema` — type definitions and utilities
- `crates/quarto-hub` — Rust sync server (also handles filesystem watching)
- Existing MCP plan docs (`claude-notes/quarto-mcp-*`) — focused on filesystem-based Quarto operations, not automerge

## Design Questions & Decisions

### Q1: Implementation Language — TypeScript or Rust?

**Decision: TypeScript**, as a standalone ts-package with its own npx-executable entry point.

Rationale:
- `quarto-sync-client` already exists and provides the entire automerge integration
- MCP SDK (`@modelcontextprotocol/sdk`) has mature TypeScript support
- Minimizes new code — the MCP server is essentially a thin adapter
- Lives in `ts-packages/quarto-hub-mcp`
- Standalone TS binary avoids needing to bridge from the Rust `quarto` CLI
- This MCP server is scoped to file access only; when Quarto 2 has full render/validate/lint support, a more comprehensive MCP server may be built (possibly in Rust as `quarto hub mcp`), potentially replacing this one

### Q2: MCP Transport — stdio vs SSE/Streamable HTTP?

**Recommendation: Start with stdio, add SSE later.**

- **stdio**: Standard for local MCP servers. Agent launches the MCP server process, passes sync server URL + doc ID as CLI args or env vars. Simple, works with Claude Code and Cursor today.
- **SSE/HTTP**: Would allow a centrally-hosted MCP server that agents connect to remotely. More complex but enables scenarios where the agent has no local Node.js runtime. Natural evolution for later.

With stdio, a typical Claude Code configuration would look like:
```json
{
  "mcpServers": {
    "quarto-hub": {
      "command": "npx",
      "args": ["@quarto/hub-mcp", "--server", "https://hub.example.com", "--doc", "abc123"]
    }
  }
}
```

### Q3: Connection Parameters

Since the server supports multiple projects, the sync server URL is a server-level config while project connections happen per-tool-call:

**Server URL**: CLI arg or env var
```bash
quarto-hub-mcp --server https://sync.example.com
# or
QUARTO_HUB_SERVER=https://sync.example.com quarto-hub-mcp
```

**Project (index doc ID)**: Passed per tool call. The server lazily connects to projects as they're referenced. A `connect_project` tool explicitly establishes a connection; subsequent file operations include the doc ID to identify which project they target.

CLI args take precedence over env vars.

### Test Sync Server

For testing, use the public automerge sync server:
- **URL**: `wss://sync.automerge.org`
- **Hello world project index doc ID**: `automerge:2knrbhSpo36X5Kk6ADkAX6qZLnfM`

### Q4: Authentication

The hub server supports OIDC auth with HttpOnly cookies. For MCP access:

- **Phase 1**: Assume the sync server is accessible without auth (local dev, `--allow-insecure-auth`)
- **Phase 2**: Support auth token / API key passed via env var or CLI arg
- **Phase 3**: Full OIDC flow (likely needs a browser redirect, complex for headless agents)

### Q5: Tool Design

What MCP tools should be exposed?

#### Core Operations (Phase 1)

All file tools take a `project` parameter (the index document ID) to identify the target project.

| Tool | Description |
|------|-------------|
| `connect_project` | Connect to a project by index doc ID. Returns file listing. Lazy — also happens implicitly on first file operation. |
| `create_project` | Create a new empty project on the sync server. Returns the new index doc ID. |
| `list_files` | List all files in the project with their types (text/binary) |
| `read_file` | Read a text file's content |
| `write_file` | Replace a text file's entire content (creates if doesn't exist) |
| `patch_file` | Apply a targeted edit to a text file (old_string → new_string), saving context window vs full-file writes |
| `create_file` | Create a new text file |
| `delete_file` | Delete a file |
| `rename_file` | Rename/move a file |

Note on `write_file` vs `patch_file`: Both go through automerge's `updateText()` which computes a string diff internally, so the sync efficiency is the same. The benefit of `patch_file` is **context window savings** — the agent only sends the changed portion rather than the entire file content in the tool call.

In `--read-only` mode, only `connect_project`, `list_files`, and `read_file` are available.

#### Extended Operations (Phase 2)

| Tool | Description |
|------|-------------|
| `read_binary_file_metadata` | Get metadata for a binary file (mime type, hash, size) |
| `create_binary_file` | Upload a binary file (base64 encoded) |
| `get_project_info` | Get project metadata (from \_quarto.yml if present) |
| `search_files` | Search file contents (grep-like, implemented client-side) |

#### Quarto-Specific Operations (Phase 3)

| Tool | Description |
|------|-------------|
| `render_preview` | Trigger a render and return HTML (requires WASM or hub with render capability) |
| `validate_document` | Validate QMD syntax |
| `get_document_outline` | Parse QMD and return section structure |

### Q6: Resource Design (MCP Resources)

Deferred. MCP resources provide read-only subscribable context, but current AI agents primarily use tools. Resources may be more relevant for IDE integrations in the future.

### Q7: Change Notifications

Deferred. Automerge supports real-time change notifications and MCP supports resource subscriptions, but this adds complexity for limited current benefit. Could be interesting for future agentic use cases (e.g., an agent monitoring a project for changes by collaborators).

## Architecture

```
┌─────────────────┐     stdio/SSE      ┌──────────────────┐     WebSocket     ┌─────────────────┐
│                 │ ◄──────────────────►│                  │◄────────────────►│                 │
│   AI Agent      │   MCP Protocol     │  Hub MCP Server  │  Automerge Sync  │  Quarto Hub     │
│  (Claude Code)  │   (JSON-RPC)       │  (TypeScript)    │                  │  (Sync Server)  │
│                 │                    │                  │                  │                 │
└─────────────────┘                    └──────────────────┘                  └─────────────────┘
                                              │
                                              │ uses
                                              ▼
                                       ┌──────────────────┐
                                       │ quarto-sync-     │
                                       │ client           │
                                       │ (existing pkg)   │
                                       └──────────────────┘
```

## Implementation Plan

### Phase 1: Minimal Viable MCP Server

- [x] Research MCP SDK TypeScript API and server setup patterns
- [x] Create new `ts-packages/quarto-hub-mcp` package
- [x] Implement CLI entry point with arg parsing (`--server`, `--read-only`)
- [x] Multi-project connection manager (lazy connect, maintains sync client instances per project)
- [x] Implement project tools: `connect_project`, `create_project`
- [x] Implement file tools: `list_files`, `read_file`, `write_file`, `patch_file`, `create_file`, `delete_file`, `rename_file`
- [x] Add MCP tool annotations (`readOnlyHint`, `destructiveHint`)
- [x] Implement `--read-only` mode (suppress write tools)
- [x] Manual stdio testing (initialize, tools/list, connect_project, read_file, error cases, --read-only)
- [ ] Test with Claude Code (configure as MCP server in .mcp.json, verify tool calls work)
- [x] Write automated tests (22 tests: protocol, read-only mode, live connect/read, live create/mutate)

### Phase 2: Polish & Extended Features

- [ ] Add binary file support (metadata reading, base64 upload)
- [ ] Add `search_files` tool (client-side grep over synced files)
- [ ] Add `get_project_info` tool
- [ ] Error handling and edge cases (disconnection, reconnection)
- [ ] Documentation

### Phase 3: Future Enhancements

- [ ] Integrate WASM parser for document validation/outline
- [ ] Add render preview capability (if hub supports it)
- [ ] SSE/HTTP transport option for remote access
- [ ] Change notifications (when agentic use cases demand it)

## Resolved Questions

1. **Package naming**: Standalone TS package in `ts-packages/quarto-hub-mcp`, executable via npx. Not a `quarto` CLI subcommand — avoids Rust↔TS bridging and keeps scope focused.

2. **Scope of "write"**: Both `write_file` (full replacement) and `patch_file` (targeted edit). Both use `updateText()` under the hood, but `patch_file` saves agent context window by only sending the diff in the tool call.

3. **Rendering integration**: This MCP server is file-access only. When Quarto 2 has render/validate/lint capabilities, a more comprehensive MCP may be built, possibly replacing this one.

4. **Project creation**: Yes. `quarto-sync-client` already supports `createNewProject()`, so we expose it.

5. **Multi-project**: Yes. Each tool call includes the index document ID, so the server can maintain multiple sync client instances. This is "free" architecturally and matches what's possible with filesystem access. The server connects to projects on demand and maintains the connections.

6. **Read-only mode**: Supported via `--read-only` CLI flag. When set, write/create/delete/rename tools are either not registered or return errors. Additionally, all tools use MCP tool annotations (`readOnlyHint: true` / `destructiveHint: true`) as hints to clients, though these aren't enforceable. Note: MCP has no standard protocol-level permission forwarding like Claude Code's built-in approval prompts. MCP's "Elicitation" feature allows servers to pause and ask users questions, but client support is still limited.

7. **Rate limiting**: Not needed for now.
