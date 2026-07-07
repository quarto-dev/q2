#!/bin/bash

# Post-tool-use hook: remind to run the changelog WASM render test after
# editing hub-client/changelog.md.
#
# The changelog is rendered through the qmd pipeline in the About tab, and
# `changelogRender.wasm.test.ts` gates it in CI (the TS Test Suite). A stray
# qmd delimiter (~ _ ^ $) produces a parse error such as
# "[Q-2-17] Unclosed Subscript", which turns the whole suite red — with the
# render succeeding for every other file, so it is easy to miss locally.
#
# This hook does NOT run the test (that would add latency and needs a built
# WASM); it injects a reminder with the exact command so the edit is not
# pushed unverified. See claude-notes/plans/2026-07-07-broken-main-ci-changelog-subscript.md
# (strand bd-q5o7ekzn) for the incident that motivated it.
#
# Receives JSON on stdin with tool_input.file_path.

file_path=$(jq -r '.tool_input.file_path // empty' 2>/dev/null)

# Only fire for hub-client/changelog.md (absolute or relative path).
case "$file_path" in
    *hub-client/changelog.md) ;;
    *) exit 0 ;;
esac

jq -n '{
  hookSpecificOutput: {
    hookEventName: "PostToolUse",
    additionalContext: "Reminder: hub-client/changelog.md is rendered through the qmd pipeline and is gated in CI by changelogRender.wasm.test.ts. A lone qmd delimiter (~ _ ^ $) will fail parsing (e.g. [Q-2-17] Unclosed Subscript) and turn the TS Test Suite red. Before committing this change, run:  cd hub-client && npm run test:wasm  — no WASM rebuild is needed for a changelog-only edit; the test runs against the existing WASM artifact."
  }
}'

exit 0
