#!/usr/bin/env bash
# Minimal repro for issue #173.
# A CodeBlock whose AST text ends with `\n` loses that newline on round-trip
# through the qmd writer.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPRO="$HERE/repro.qmd"
cd "$HERE"/../../..

echo "=== input bytes ==="
od -c "$REPRO" | head -3

echo
echo "=== first parse (text should be \"foo\\n\") ==="
cargo run --quiet --bin pampa -- "$REPRO"

echo
echo "=== qmd writer output (closing fence is glued to 'foo\\n', BUG) ==="
cargo run --quiet --bin pampa -- "$REPRO" -t qmd | od -c | head -3

echo
echo "=== round-trip parse (text is now \"foo\", BUG — trailing \\n lost) ==="
cargo run --quiet --bin pampa -- "$REPRO" -t qmd | cargo run --quiet --bin pampa --
