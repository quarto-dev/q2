# Script Dependencies

This document lists all external dependencies required by scripts in this directory.

## audit-error-codes.py

### Required

#### ripgrep (rg)
**Version:** Any recent version (tested with 13.0+)
**Purpose:** Fast code searching across the repository

**Why required:** The script uses `rg --json` for efficient searching of error codes across thousands of files. Ripgrep is significantly faster than `grep` and provides structured JSON output.

**Installation:**

| Platform | Command |
|----------|---------|
| macOS (Homebrew) | `brew install ripgrep` |
| Ubuntu/Debian | `apt install ripgrep` |
| Fedora | `dnf install ripgrep` |
| Arch Linux | `pacman -S ripgrep` |
| Windows (Chocolatey) | `choco install ripgrep` |
| Windows (Scoop) | `scoop install ripgrep` |
| Cargo (any platform) | `cargo install ripgrep` |

**Official installation guide:** https://github.com/BurntSushi/ripgrep#installation

**Verification:**
```bash
rg --version
# Should output: ripgrep X.Y.Z
```

**Troubleshooting:**
- If `rg` is not in PATH, ensure your package manager's bin directory is in PATH
- On some systems, ripgrep might be installed as `rg` in `/usr/local/bin/`
- If using Cargo, ensure `~/.cargo/bin` is in your PATH

#### Python 3.6+
**Version:** 3.6 or later
**Purpose:** Script runtime

**Built-in modules used:**
- `argparse` - Command-line parsing
- `json` - JSON parsing/generation
- `re` - Regular expressions
- `subprocess` - Running ripgrep
- `pathlib` - File path handling
- `dataclasses` - Data structure definitions (3.7+, backport available)

**Installation:**
Most systems have Python 3 pre-installed. Check with:
```bash
python3 --version
```

If not installed:
- macOS: `brew install python3` or use built-in version
- Ubuntu/Debian: `apt install python3`
- Windows: Download from https://www.python.org/downloads/

### Optional

None. The script has no optional dependencies.

## quick-error-audit.sh

### Required

#### ripgrep (rg)
Same as above.

#### jq
**Version:** Any recent version
**Purpose:** JSON parsing in bash

**Installation:**
```bash
brew install jq           # macOS
apt install jq            # Ubuntu/Debian
dnf install jq            # Fedora
```

#### Standard Unix tools
- `bash` (4.0+)
- `grep`
- `wc`
- `sort`
- `comm`
- `cat`

These are pre-installed on macOS/Linux.

## beads-to-graphviz.py

### Required

See `README-beads-graphviz.md` for details.

## Checking All Dependencies

Run this to check all dependencies at once:

```bash
#!/bin/bash
echo "Checking script dependencies..."
echo

# ripgrep
if command -v rg &> /dev/null; then
    echo "✅ ripgrep: $(rg --version | head -1)"
else
    echo "❌ ripgrep: NOT FOUND"
    echo "   Install: brew install ripgrep"
fi

# Python 3
if command -v python3 &> /dev/null; then
    echo "✅ Python 3: $(python3 --version)"
else
    echo "❌ Python 3: NOT FOUND"
fi

# jq (optional, for bash scripts)
if command -v jq &> /dev/null; then
    echo "✅ jq: $(jq --version)"
else
    echo "⚠️  jq: NOT FOUND (only needed for bash scripts)"
fi

echo
echo "Required: ripgrep, Python 3"
echo "Optional: jq (for quick-error-audit.sh)"
```

Save as `scripts/check-dependencies.sh` and run:
```bash
chmod +x scripts/check-dependencies.sh
./scripts/check-dependencies.sh
```

## Why ripgrep?

**Performance:**
- 10-100x faster than grep on large codebases
- Respects `.gitignore` automatically
- Parallel searching across files

**Features:**
- `--json` output for structured parsing
- Multi-line search support
- Context lines (`-A`, `-B`, `-C`)
- Type filtering (`--type rust`, `--type json`)
- Glob patterns (`--glob`)

**Alternatives considered:**
- `grep -r`: Too slow, no structured output
- `ag` (The Silver Searcher): Good but ripgrep is faster
- Python's built-in file walking: Too slow for large repos

**Fallback:** Not provided. If ripgrep is not available, the script will fail with a clear error message directing to installation instructions.
