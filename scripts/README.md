# Scripts Directory

This directory contains automation scripts for the Kyoto project.

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
- Finds all Q-*-* error codes in the codebase (crates + private-crates)
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

## Beads/Issue Tracking

### beads-to-graphviz.py, beads-to-graphviz.sh

**Purpose:** Visualize beads issue dependencies as graphs.

See: `README-beads-graphviz.md` for details.

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
