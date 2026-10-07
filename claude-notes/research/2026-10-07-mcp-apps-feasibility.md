# MCP Apps (`io.modelcontextprotocol/ui`) feasibility — in-host Quarto preview

Phase 6 "explore (no commitment)" item of
[2026-10-05-elevate-quarto-hub-mcp.md](../plans/2026-10-05-elevate-quarto-hub-mcp.md)
(bd-8iv9jty5). One page, per the plan.

## What it would be

An MCP Apps extension surface on the hub-mcp server that renders a
project file (or the rendered site page) as an interactive HTML view
**inside the host's conversation** — the closest MCP equivalent of the
web client's live preview, aimed at the human half of the
human+agent pair watching what the agent changed.

Mechanically, MCP Apps (shipped in `@modelcontextprotocol/ext-apps`
2.0.x, already in our tree transitively via the inspector) lets a tool
result carry a `ui://` resource link; the host renders that resource in
a sandboxed iframe and bridges host theming/tool-call context into it.
A tool like `preview_file({path})` would return text ("preview
attached") plus a `ui://preview/<path>` resource the host displays.

## What it costs (rough)

- A UI resource pipeline: qmd → HTML fragment (we have the render
  path), asset inlining (no external fetches inside the sandbox),
  CSP-safe markup, plus the host-bridge glue (theme, resize, tool-call
  round-trips if the preview is interactive).
- A new protocol surface to keep conformant (the extension has its own
  conformance scenarios — all `extension`/not-scored today).
- Careful trust framing: project content is multi-author; a rendered
  preview is executable-adjacent HTML inside the host. The sandbox +
  our existing untrusted-content guidance carry most of it, but the
  iframe boundary needs an explicit review pass.

## Host support (the gate)

MCP Apps renders inline "in Claude, ChatGPT and any other compliant
chat client" per the ext-apps README, with vendor docs live (Claude's
connector docs ship a getting-started). Support is real but early:
flags/rollout vary by host and surface (web vs desktop vs mobile), and
the extension's conformance scenarios are all not-scored
(`extension`) in the frozen 2026-07-28 requirements — the spec
explicitly does not require it for any tier.

## Recommendation

**Worth doing, not yet.** The value is real (it is the only plausible
"show the human what changed" channel for stdio-less hosts), the build
is a well-bounded chunk (the preview render already exists; the work
is the ui:// surface + sandboxing), and host support is clearly
trending up. But committing now buys a feature whose reach depends on
per-host rollout we don't control, on an extension the frozen
requirement sets still treat as optional. Trigger to revisit: a
mainstream host advertising MCP Apps support as GA on both web and
desktop, or the extension's scenarios moving into a scored
requirements set — at that point a `preview_file` tool with a
`ui://` result is a small, high-leverage addition. Filed as a watch
item on bd-8iv9jty5; no code in this phase.
