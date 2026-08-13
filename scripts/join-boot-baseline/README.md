# Join-boot baseline harness

Measurement harness for the live-share join payload (plan
`claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`, Phase 0 /
bd-lbvtfejg). Phase 1 re-runs these measurements against the same
fixture to decide the Phases 2–3 gate (target: ≤ 5 s first render over
the simulated slow link).

## Files

- `boot-driver.mjs` — headless-Chromium boot capture. Records every
  request the preview SPA issues (method, path, status, bytes from
  `content-length` or the body; `blob:` URLs are reported separately as
  local), the `/ws` upgrade, and time-to-first-render (polls all frames
  for the fixture's `MARKER-0` — the SPA renders inside an iframe).
  Usage: `node boot-driver.mjs <url> <report.json>` (run from the repo
  root so `playwright` resolves).
- `throttle-proxy.mjs` — no-deps, no-sudo slow-link simulator. One
  *shared* downstream token bucket across all connections (a real link
  is shared; per-connection buckets would multiply bandwidth by
  Chromium's connection count) plus a fixed one-way delay each way.
  Usage: `node throttle-proxy.mjs <listen-port> <target-port> [rate-mbps] [rtt-ms]`
  (defaults 10 Mbps / 100 ms).

## Fixture

Three files in any scratch dir:

```sh
printf 'project:\n  type: website\n' > _quarto.yml
printf '# Index\n\nMARKER-0\n' > index.qmd
printf '# About\n' > about.qmd
```

## Legs (as run for the 2026-08-13 baseline)

a. **Direct**: `target/debug/q2 preview <fixture> --no-browser --port 9377`,
   driver against `http://127.0.0.1:9377/?page=index.qmd`.
b. **Relay-pinned**: Gate 0 spike pair in front of the same server —
   `spike-tunnel-host 127.0.0.1:9377`, then
   `spike-tunnel-client <TICKET> <TOKEN> 9280 --relay-only` (binaries in
   `.worktrees/bd-l4j4ky8k-live-share-feasibility-gate/target/debug/`;
   the real `--join` has no relay-pinning knob), driver against 9280.
   Confirm `0` DIRECT selections in the client log.
c. **Slow link**: real stack — `q2 preview <fixture> --share
   --no-browser --port 9378`, `q2 preview --join <ticket> --no-browser
   --port 9281`, `throttle-proxy.mjs 9282 9281 10 100`, driver against
   9282. (The slow link sits between browser and guest proxy; the
   tunnel is byte-transparent, so this is what the browser experiences
   on a residential-class link. The CLI's tiny `/health` + config
   preflight runs unthrottled — negligible bytes.)

On-demand fonts: add math to `index.qmd` (e.g. `$E=mc^2$`) and re-run
leg (a) to see the KaTeX woff2 fetches.
