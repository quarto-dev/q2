#!/usr/bin/env bash
# Local repro attempt for #1266: build the VITE_E2E harness bundle the way
# hub-client-e2e.yml does, then run pandoc-warm in WebKit with the skip lifted.
# Run from the repo root. Pass extra playwright args (e.g. `-g "A,B,A"`).
set -euo pipefail
cd hub-client
# (build:wasm done separately)
node ../scripts/fetch-pandoc-wasm.mjs --require
for pkg in ../ts-packages/*/; do
  [ -f "$pkg/package.json" ] && (cd .. && npm run build --workspace "${pkg#../}" --if-present)
done
VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws \
  VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html \
  npm run build:sandboxed
VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws \
  VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html \
  npm run build
# The spec's top-level test.skip for webkit must be removed (or overridden) first.
ISSUE_1266=1 npx playwright test --config playwright.harness.config.ts \
  e2e/pandoc-warm.harness.spec.ts --project=webkit --retries=0 --workers=1 "$@"
