#!/bin/bash
# Code coverage script for the Quarto Rust workspace
# Requires: cargo-llvm-cov (cargo install cargo-llvm-cov)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
COVERAGE_DIR="$PROJECT_ROOT/coverage"

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
    --html          Generate HTML report (default)
    --lcov          Generate lcov.info file
    --json          Generate JSON report
    --summary       Show summary only (no report files)
    --open          Open HTML report in browser after generation
    -h, --help      Show this help message

Examples:
    $(basename "$0")              # Generate HTML report
    $(basename "$0") --summary    # Quick summary
    $(basename "$0") --lcov       # Generate lcov for CI
    $(basename "$0") --html --open # Generate and open HTML report
EOF
}

OUTPUT_MODE="html"
OPEN_REPORT=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --html)
            OUTPUT_MODE="html"
            shift
            ;;
        --lcov)
            OUTPUT_MODE="lcov"
            shift
            ;;
        --json)
            OUTPUT_MODE="json"
            shift
            ;;
        --summary)
            OUTPUT_MODE="summary"
            shift
            ;;
        --open)
            OPEN_REPORT=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

cd "$PROJECT_ROOT"

echo "Running code coverage for workspace..."

case $OUTPUT_MODE in
    html)
        cargo llvm-cov nextest --workspace --html --output-dir "$COVERAGE_DIR"
        echo ""
        echo "HTML report generated: $COVERAGE_DIR/html/index.html"
        if $OPEN_REPORT; then
            open "$COVERAGE_DIR/html/index.html"
        fi
        ;;
    lcov)
        cargo llvm-cov nextest --workspace --lcov --output-path "$COVERAGE_DIR/lcov.info"
        echo ""
        echo "lcov report generated: $COVERAGE_DIR/lcov.info"
        ;;
    json)
        cargo llvm-cov nextest --workspace --json --output-path "$COVERAGE_DIR/coverage.json"
        echo ""
        echo "JSON report generated: $COVERAGE_DIR/coverage.json"
        ;;
    summary)
        cargo llvm-cov nextest --workspace
        ;;
esac
