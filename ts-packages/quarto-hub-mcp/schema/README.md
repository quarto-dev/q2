# Vendored MCP Registry schema

`server.schema.json` is a pinned copy of the official MCP Registry
`server.json` schema:

- Source: <https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json>
- Fetched: 2026-10-07
- Schema date: 2025-12-11

It exists so `src/registry.test.ts` can validate `../server.json`
offline (the repo's test policy is offline-by-default). Refresh it
deliberately — when the registry announces a schema revision the listing
needs — with:

```sh
curl -sS https://static.modelcontextprotocol.io/schemas/<date>/server.schema.json \
  -o schema/server.schema.json
```

and update this README in the same commit. `mcp-publisher publish`
performs its own server-side validation at publish time; this copy is
the pre-flight net, not the authority.
