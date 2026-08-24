# Scripts Directory

This directory contains automation scripts for the Kyoto project.

## Local Production Mode

Two modes available:

**`local-prod.sh`** (Node.js proxy) - Quick setup, no dependencies
**`local-prod-nginx.sh`** (nginx in Docker) - Test actual nginx config

### What it does

Mirrors the production deployment architecture:
- Starts the `hub` binary (Rust server) on port 3000
- Serves the built hub-client on port 8080
- Serves q2-sandboxed-preview.html on port 8081 (sandboxed, simulates separate domain)
- Proxies `/auth` and `/ws` requests to the hub server
- Uses production-like cache headers

### Usage

```bash
# Prerequisites
cargo build --bin hub
cd hub-client && npm run build:local-prod && cd ..

# Run (from hub-client directory)
cd hub-client
npm run local-prod

# Use a different port (pass it to both build and run)
npm run build:local-prod -- --port 9000
npm run local-prod -- --port 9000

# Or run directly
./scripts/local-prod.sh
```

**Important:** Use `npm run build:local-prod` (NOT `build:all`) to build with the local sync server URL baked in. If using a non-default port, pass `--port PORT` to both `build:local-prod` and `local-prod`.

Open `http://127.0.0.1:8080` in your browser. Press **Ctrl-C** to shut down gracefully.

### Architecture

```
Browser → http://127.0.0.1:8080 (Node.js proxy - main app)
  ├─ /auth → proxy to hub:3000
  ├─ /ws → WebSocket upgrade to hub:3000
  ├─ /assets/* → serve from dist/ (immutable cache)
  └─ /* → serve from dist/ (no-cache, SPA fallback)

Browser → http://127.0.0.1:8081 (q2-sandboxed-preview server - sandboxed)
  └─ /q2-sandboxed-preview.html → sandboxed AST renderer (separate origin)
  
hub:3000 (Rust binary) → .local-prod-data/
```

**Why q2-sandboxed-preview.html on a separate port?**

In production, q2-sandboxed-preview.html will be served from a separate domain (e.g., `raw.quarto.pub`) for security isolation. The separate port in local-prod simulates this cross-origin setup. The iframe uses `sandbox="allow-scripts allow-same-origin"` and communicates via postMessage.

### Node.js Proxy Mode (Recommended)

**Prerequisites:** None (just Node.js, already required)

```bash
cd hub-client
npm run local-prod
```

Fast setup, tests WebSocket proxying and routing. Good for 90% of development.

### Nginx Mode

**Prerequisites:** Docker Desktop

```bash
cd hub-client
npm run local-prod:nginx
```

Tests the actual nginx configuration from production. Use when:
- Testing nginx config changes before deploying
- Validating gzip compression, security headers
- Debugging nginx-specific issues

**Architecture differences:**
- **Node.js mode:** Browser → Node.js proxy (port 8080) → hub (port 3000)
- **Nginx mode:** Browser → nginx (Docker, port 8080) → hub (host, port 3000)
- **Production:** Browser → nginx (native) → hub (native, port 3000)

**Differences from production:** HTTP (no TLS), no OIDC auth, single-machine.

**Logs:**
- Node.js mode: `.local-prod-data/{hub,static,q2-sandboxed-preview}.log`
- Nginx mode: `.local-prod-data/{hub,q2-sandboxed-preview}.log` + `docker compose -f docker-compose.local-prod.yml logs nginx`

**See also:** [`claude-notes/plans/2026-07-03-local-prod-mode.md`](../claude-notes/plans/2026-07-03-local-prod-mode.md) for implementation details.

## Error Code Auditing

### audit-error-codes.py ⭐ RECOMMENDED

**Purpose:** Automated auditing of error code consistency between `error_catalog.json` and source code.

**Requirements:**
- **Python 3.6+** (usually pre-installed)
- **ripgrep (`rg` command)** - REQUIRED
  - Install: https://github.com/BurntSushi/ripgrep#installation
  - macOS: `brew install ripgrep`
  - Ubuntu: `apt install ripgrep`
  - Fedora: `dnf install ripgrep`
  - Windows: `choco install ripgrep`

**Features:**
- Finds all Q-*-* error codes in the codebase
- Identifies missing catalog entries (HIGH PRIORITY)
- Identifies orphaned catalog entries (unused codes)
- Automatically categorizes codes:
  - **Legitimate missing** - Used in production code → Add to catalog
  - **Test/Example codes** - Only in tests/docs → Document or update examples
  - **Invalid format** - Typos, test sentinels → Investigate
- Detects format issues (leading zeros, invalid subsystems)
- Multiple output formats (text, JSON, markdown)
- Subsystem breakdown (Q-0, Q-1, Q-2, Q-3)

**Quick Setup Check:**
```bash
# Run the dependency checker (recommended)
./scripts/check-dependencies.sh

# Or check manually:
python3 --version  # Should be 3.6+
rg --version       # Should show ripgrep version

# If rg is missing, install it:
brew install ripgrep  # macOS
```

**Usage:**
```bash
# Quick text report to terminal
./scripts/audit-error-codes.py

# JSON for tooling/CI integration
./scripts/audit-error-codes.py --format json > audit.json

# Markdown report to file
./scripts/audit-error-codes.py --format markdown -o docs/audit-report.md

# Specify custom repo root
./scripts/audit-error-codes.py --repo-root /path/to/repo
```

**Ignore Feature:**

Exclude error codes from audit results:

**Line-level:** Add `quarto-error-code-audit-ignore` on the same line
```rust
assert_eq!(get_subsystem("Q-999-999"), None); // quarto-error-code-audit-ignore
```

**File-level:** Add `quarto-error-code-audit-ignore-file` anywhere in the file (usually at top)
```markdown
<!-- quarto-error-code-audit-ignore-file -->
# Design doc with many example error codes
```

See: `claude-notes/workflows/2025-11-23-error-code-audit-ignore-feature.md`

**Output Example:**
```
============================================================
ERROR CODE AUDIT RESULTS
============================================================

SUMMARY
------------------------------------------------------------
  Codes in catalog:    67
  Codes in source:     119
  Consistent:          67 ✅
  Missing (catalog):   52 ❌
    - Legitimate:      9 (HIGH PRIORITY)
    - Test/Examples:   42 (LOW PRIORITY)
    - Invalid format:  1 (INVESTIGATE)
  Orphaned (unused):   0 ✅

LEGITIMATE MISSING CODES (HIGH PRIORITY)
------------------------------------------------------------
  • Q-1-90
    Occurrences: 20
    Files: 3
    First use: private-crates/quarto-yaml-validation/src/error.rs:175
  ...
```

**Exit codes:**
- 0: No issues found (all codes consistent)
- 1: Missing or orphaned codes detected (action needed)

**See also:**
- Workflow: `claude-notes/workflows/2025-11-23-error-code-audit-workflow.md`
- Latest results: `claude-notes/investigations/2025-11-23-error-code-audit-results.md`
- Fix guide: `claude-notes/investigations/2025-11-23-add-missing-catalog-entries.md`

### quick-error-audit.sh

**Purpose:** Simple bash version of error code audit.

**Usage:**
```bash
./scripts/quick-error-audit.sh
```

**Recommendation:** Use `audit-error-codes.py` instead for:
- Better categorization (legitimate vs test/example codes)
- Multiple output formats
- More detailed analysis

## Dependency Checking

### check-dependencies.sh

**Purpose:** Verify all required dependencies are installed.

**Usage:**
```bash
./scripts/check-dependencies.sh
```

**Checks:**
- ✅ ripgrep (required for audit-error-codes.py)
- ✅ Python 3.6+ (required for Python scripts)
- ℹ️  jq (optional, for bash scripts)

Run this before using any scripts to ensure dependencies are met.

## Contributing New Scripts

When adding new scripts to this directory:

1. **Make executable:** `chmod +x script-name`
2. **Add shebang:** `#!/usr/bin/env python3` or `#!/bin/bash`
3. **Add to this README** with purpose, usage, and examples
4. **Document in claude-notes/** if it relates to a workflow
5. **Consider:**
   - Error handling
   - Help text (`--help`)
   - Exit codes (0 = success, 1+ = error)
   - JSON output for tooling integration
