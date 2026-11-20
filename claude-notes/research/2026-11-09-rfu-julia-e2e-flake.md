---
title: 'RFU-5: the Julia e2e EPIPE comes from a killed client, not from the server'
date: 2026-11-09
description: 'Three clean runs of the Positron fork''s q2 e2e file passed 5 of 5 with no EPIPE in the Julia server log. Killing q2 and its Node child mid-run reproduces the EPIPE exactly, so a hung-up client is sufficient. The earlier 120 s runs with no outputs were most likely a test timeout whose teardown killed the client, with a cold or stale server as the underlying slowness.'
---

**Plan:** [RFU-5](../plans/2026-11-08-rfu-5-julia-e2e-flake.md). **Fork:** `/Users/gordon/src/positron-riu6-a` (read and run
only; nothing was committed to it).

## Summary

- The EPIPE in the Julia server log is what the server prints when the client socket is gone while it writes a
  response. Killing the client is **sufficient** to produce it (experiment 1, ran).
- Three full runs of `quarto-render-and-push.test.ts` passed 5 of 5 each (ran), and none of the three server logs
  contains an EPIPE. So a test that passes never kills its client, and the 120 s failures from the earlier session
  were not reproduced.
- Most likely cause of those failures (**inferred, not proven**): the Julia tests started against a cold or stale server,
  ran past the then-default Playwright test timeout, and Playwright\'s teardown killed Positron and with it the extension
  host\'s `q2` and its Node child. The EPIPE is the symptom of that kill, not the cause of the slowness. The fork change
  `53bf9ca105` (warm-up plus a 480 s test timeout) removes that path, and the three passing runs are consistent with it.

## Runs

The runs used a `q2` that still has `q2 record export-positron` (copied from `workspace-6`), because the fork calls
that subcommand and the current checkout\'s `q2` has no `record` command after RFU-1 (read; ran `q2 record --help`).
Environment: pinned Node 24.22.1, a scratch venv with ipykernel, matplotlib and pandas (Python 3.13.9), a Julia 1.12.7
project with Plots, Positron from `.build/electron`.

| Run | Result | Wall time |
|---|---|---|
| 1 | 5 passed | 4.6 min |
| 2 | 5 passed | 3.7 min |
| 3 | 5 passed | 4.0 min |

Server log after each run: two lines (`Log started`, `Starting server`), no error, because one server stayed up for
all three runs (started 05:15:46, before run 1). Logs were copied to the session scratchpad after each run.

**Two false starts, both mine, not the flake.** The first attempt failed at config load (the system Node 24.5.0 could not
resolve `./lanes.js`; the pinned 24.22.1 can). The second ran and failed both Julia tests, because I had copied a Julia
project whose `Manifest.toml` was resolved for Julia 1.11.7: the warm-up export failed with "Julia version mismatch in
notebook file" (shown by running the warm-up command by hand, since the helper uses `stdio: 'ignore'` and hides stderr).
A fresh `Pkg.add("Plots")` on 1.12.7 fixed it. Worth noting for RFU-7: the warm-up\'s silent `stdio: 'ignore'` turned a
clear message into a bare "Command failed".

## Experiments

1. **Kill the client mid-run (ran).** In a scratch directory, with the Julia project beside it, `q2 record export-positron`
   on a document with `sleep(25)` and `sleep(5)` cells. After 8 s, `kill -9` on the `q2` process and its Node child (the
   `engine-host-node` bundle that holds the socket). About 40 s later the server log showed
   `Unhandled Task ERROR: IOError: write: broken pipe (EPIPE)` from `_write_json` in `socket_json.jl`, reached from
   `_handle_response_internal`. A hung-up client is sufficient, and the server survives it (the server process kept
   running). A first attempt on the stock fixture finished before the kill (a warm server runs it in seconds), which is
   why the document sleeps.
2. **Match an EPIPE timestamp to per-test end times (not done).** No EPIPE occurred in any of the three passing runs, so
   there was nothing to match. This needs a run that actually times out.
3. **Look for a second q2 run in flight at teardown (partly done).** No orphan `q2` was found after the passing runs. The
   machine does have several Julia servers sharing one transport file and one log
   (`~/Library/Caches/quarto/julia/julia_transport.txt`, `julia_server_log.txt`): the one used by these runs plus two
   older ones (started Nov 3 and Nov 5) and, at times, another lane\'s. They belong to other work and were not touched.
   A client attaches through the transport file, so a stale or foreign server there is a plausible source of a slow or
   hung first run; this is **not tested**.
4. **The extension\'s `execFile` timeout (read).** `exportCommand.ts` uses 300 s (`q2.exportTimeoutSeconds`), which is
   longer than the tests\' waits, so it is not what killed the client in a 120 s failure. Playwright\'s test timeout and
   worker teardown are the remaining candidate; q2\'s own kill path in `ts_process.rs` was not read for this note.

## Proposed fork changes for RFU-7 (not committed)

1. Do not hide the warm-up\'s stderr, so a Julia version mismatch or a missing package says so:

   ```diff
   -	execFileSync(q2Path, [...], { cwd: dir, timeout: 300000, stdio: 'ignore' });
   +	execFileSync(q2Path, [...], { cwd: dir, timeout: 300000, stdio: ['ignore', 'ignore', 'inherit'] });
   ```

2. Keep `warmJuliaServer` and the 480 s timeout from `53bf9ca105`. They are the measured fix for the cold start.
3. Document in the run guide that `Q2_E2E_JULIA_PROJECT` needs a `Manifest.toml` resolved with the Julia on `PATH`.
4. When the flake recurs, copy `julia_server_log.txt` and note the test\'s end time first: an EPIPE at the same moment as
   a test timeout confirms this note\'s cause; no EPIPE would mean the closer is elsewhere.

## What this does not settle

The 120 s failures were not reproduced, so the cause above is an inference from experiment 1 plus the timing of the
earlier report. No q2 code change is proposed: nothing here implicates `ts_process.rs`.
