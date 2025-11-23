#!/bin/bash
# Check dependencies for scripts in this directory

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Script Dependencies Check"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

all_ok=true

# ripgrep (REQUIRED)
if command -v rg &> /dev/null; then
    version=$(rg --version | head -1)
    echo "✅ ripgrep: $version"
else
    echo "❌ ripgrep: NOT FOUND (REQUIRED)"
    echo "   Install: brew install ripgrep  # macOS"
    echo "           apt install ripgrep   # Ubuntu"
    echo "           dnf install ripgrep   # Fedora"
    echo "   See: https://github.com/BurntSushi/ripgrep#installation"
    all_ok=false
fi

# Python 3 (REQUIRED)
if command -v python3 &> /dev/null; then
    version=$(python3 --version)
    echo "✅ Python 3: $version"

    # Check version is 3.6+
    py_version=$(python3 -c 'import sys; print(".".join(map(str, sys.version_info[:2])))')
    if python3 -c 'import sys; exit(0 if sys.version_info >= (3, 6) else 1)'; then
        echo "   (Version OK: $py_version >= 3.6)"
    else
        echo "   ⚠️  Version $py_version is older than required 3.6"
        all_ok=false
    fi
else
    echo "❌ Python 3: NOT FOUND (REQUIRED)"
    echo "   Usually pre-installed on macOS/Linux"
    echo "   Windows: https://www.python.org/downloads/"
    all_ok=false
fi

echo

# jq (optional, for bash scripts)
if command -v jq &> /dev/null; then
    version=$(jq --version)
    echo "✅ jq: $version (optional)"
else
    echo "ℹ️  jq: NOT FOUND (optional - only needed for bash scripts)"
    echo "   Install: brew install jq"
fi

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [ "$all_ok" = true ]; then
    echo "✅ All required dependencies are installed!"
    echo
    echo "You can run:"
    echo "  ./scripts/audit-error-codes.py"
    exit 0
else
    echo "❌ Missing required dependencies"
    echo
    echo "Install missing dependencies and run this script again."
    exit 1
fi
