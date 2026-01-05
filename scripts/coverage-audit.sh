#!/bin/bash
# Coverage audit script for the Quarto Rust workspace
# Compares coverage with and without exclusions to audit #[coverage(off)] usage
#
# Requires: cargo-llvm-cov (cargo install cargo-llvm-cov)
# Works on: macOS and Linux

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Colors for output (disabled if not a terminal)
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BLUE='\033[0;34m'
    BOLD='\033[1m'
    NC='\033[0m' # No Color
else
    RED=''
    GREEN=''
    YELLOW=''
    BLUE=''
    BOLD=''
    NC=''
fi

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Audit coverage exclusion annotations by comparing coverage with and without
the #[coverage(off)] attributes. This helps identify if exclusions are being
overused or hiding important untested code.

Options:
    --threshold N   Set the gap threshold percentage (default: 5)
    --json          Output results as JSON
    --markdown      Output results as Markdown
    --quiet         Only output the final summary line
    -h, --help      Show this help message

Examples:
    $(basename "$0")                  # Run audit with default 5% threshold
    $(basename "$0") --threshold 3    # Use stricter 3% threshold
    $(basename "$0") --json           # Output JSON for scripting
    $(basename "$0") --markdown       # Output Markdown report
EOF
}

THRESHOLD=5
OUTPUT_FORMAT="text"
QUIET=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --threshold)
            THRESHOLD="$2"
            shift 2
            ;;
        --json)
            OUTPUT_FORMAT="json"
            shift
            ;;
        --markdown)
            OUTPUT_FORMAT="markdown"
            shift
            ;;
        --quiet)
            QUIET=true
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

# Helper to print progress (respects --quiet)
progress() {
    if [ "$QUIET" = false ] && [ "$OUTPUT_FORMAT" = "text" ]; then
        echo -e "${BLUE}==>${NC} $1"
    fi
}

# Helper to extract line coverage percentage from cargo llvm-cov output
extract_coverage_percent() {
    local output_file="$1"
    local total_line
    total_line=$(grep "^TOTAL" "$output_file" 2>/dev/null || echo "")

    if [ -z "$total_line" ]; then
        echo "0"
        return 1
    fi

    # Extract line coverage percentage (column 10 in the TOTAL line)
    # Format: TOTAL  files  regions  missed  cover%  functions  missed  cover%  lines  missed  cover%
    #         $1     $2     $3       $4      $5      $6         $7      $8      $9     $10     $11 (but $10 has %)
    # Actually the format varies; let's extract the percentage that appears after "lines" info
    # The line coverage % is the 4th percentage in the output
    echo "$total_line" | awk '{
        # Find all percentage values (numbers followed by %)
        for (i=1; i<=NF; i++) {
            if ($i ~ /%$/) {
                gsub(/%/, "", $i)
                print $i
                exit
            }
        }
    }'
}

# More robust extraction - get the line coverage specifically
extract_line_coverage() {
    local output_file="$1"
    # The TOTAL line format is:
    # TOTAL    [regions] [missed] [cover%]  [functions] [missed] [cover%]  [lines] [missed] [cover%]
    # We want the lines cover% which is typically the last percentage
    local total_line
    total_line=$(grep "^TOTAL" "$output_file" 2>/dev/null || echo "")

    if [ -z "$total_line" ]; then
        echo "0"
        return 1
    fi

    # Get all fields, find the pattern for lines coverage
    # Lines coverage is the third percentage value
    echo "$total_line" | awk '{
        count = 0
        for (i=1; i<=NF; i++) {
            if ($i ~ /^[0-9]+\.[0-9]+%$/ || $i ~ /^[0-9]+%$/) {
                count++
                if (count == 3) {  # Third percentage is line coverage
                    gsub(/%/, "", $i)
                    print $i
                    exit
                }
            }
        }
    }'
}

# Create temp directory for intermediate files
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT

WITH_OUTPUT="$TEMP_DIR/coverage_with.txt"
WITHOUT_OUTPUT="$TEMP_DIR/coverage_without.txt"

# Step 1: Run coverage WITH exclusions
progress "Running coverage WITH exclusions..."
if ! cargo llvm-cov nextest --workspace > "$WITH_OUTPUT" 2>&1; then
    echo -e "${RED}Error:${NC} Coverage run with exclusions failed" >&2
    cat "$WITH_OUTPUT" >&2
    exit 1
fi

COVERAGE_WITH=$(extract_line_coverage "$WITH_OUTPUT")
if [ -z "$COVERAGE_WITH" ] || [ "$COVERAGE_WITH" = "0" ]; then
    echo -e "${RED}Error:${NC} Could not extract coverage percentage" >&2
    echo "Output was:" >&2
    cat "$WITH_OUTPUT" >&2
    exit 1
fi

progress "Coverage with exclusions: ${COVERAGE_WITH}%"

# Step 2: Clean for rebuild
# IMPORTANT: cargo llvm-cov clean only removes profraw/profdata files, NOT the
# compiled binaries. The llvm-cov-target directory can be ~10GB and running twice
# with different cfg flags would double that. We must remove it entirely.
progress "Cleaning for rebuild (removing llvm-cov-target to save disk space)..."
cargo llvm-cov clean --workspace 2>/dev/null
rm -rf "$PROJECT_ROOT/target/llvm-cov-target"

# Step 3: Run coverage WITHOUT exclusions
progress "Running coverage WITHOUT exclusions..."
if ! cargo llvm-cov nextest --workspace --no-cfg-coverage-nightly > "$WITHOUT_OUTPUT" 2>&1; then
    echo -e "${RED}Error:${NC} Coverage run without exclusions failed" >&2
    cat "$WITHOUT_OUTPUT" >&2
    exit 1
fi

COVERAGE_WITHOUT=$(extract_line_coverage "$WITHOUT_OUTPUT")
if [ -z "$COVERAGE_WITHOUT" ] || [ "$COVERAGE_WITHOUT" = "0" ]; then
    echo -e "${RED}Error:${NC} Could not extract coverage percentage (without exclusions)" >&2
    echo "Output was:" >&2
    cat "$WITHOUT_OUTPUT" >&2
    exit 1
fi

progress "Coverage without exclusions: ${COVERAGE_WITHOUT}%"

# Step 4: Count exclusion annotations
progress "Counting exclusion annotations..."
EXCLUSION_COUNT=$(grep -rE "#\[(coverage\(off\)|cfg_attr\(coverage|cfg\(.*coverage)" crates/ --include="*.rs" 2>/dev/null | wc -l | tr -d ' ')
EXCLUSION_FILES=$(grep -rlE "#\[(coverage\(off\)|cfg_attr\(coverage|cfg\(.*coverage)" crates/ --include="*.rs" 2>/dev/null | sort -u || echo "")

# Step 5: Calculate gap
# Gap = with - without (exclusions inflate the "with" percentage by removing uncovered code from counting)
GAP=$(echo "$COVERAGE_WITH - $COVERAGE_WITHOUT" | bc -l 2>/dev/null || echo "0")
# Round to 2 decimal places
GAP=$(printf "%.2f" "$GAP")

# Check if gap exceeds threshold
EXCEEDS_THRESHOLD=false
if [ "$(echo "$GAP > $THRESHOLD" | bc -l)" = "1" ]; then
    EXCEEDS_THRESHOLD=true
fi

# Step 6: Output results
TOTAL_WITH=$(grep "^TOTAL" "$WITH_OUTPUT" 2>/dev/null || echo "N/A")
TOTAL_WITHOUT=$(grep "^TOTAL" "$WITHOUT_OUTPUT" 2>/dev/null || echo "N/A")

# Cross-platform date handling
if date --version >/dev/null 2>&1; then
    # GNU date (Linux)
    AUDIT_DATE=$(date -u +"%Y-%m-%d")
else
    # BSD date (macOS)
    AUDIT_DATE=$(date -u +"%Y-%m-%d")
fi

case $OUTPUT_FORMAT in
    json)
        # Count files with exclusions
        if [ -n "$EXCLUSION_FILES" ]; then
            FILE_COUNT=$(echo "$EXCLUSION_FILES" | wc -l | tr -d ' ')
        else
            FILE_COUNT=0
        fi

        cat <<EOF
{
  "date": "$AUDIT_DATE",
  "coverage_with_exclusions": $COVERAGE_WITH,
  "coverage_without_exclusions": $COVERAGE_WITHOUT,
  "gap": $GAP,
  "threshold": $THRESHOLD,
  "exceeds_threshold": $EXCEEDS_THRESHOLD,
  "exclusion_count": $EXCLUSION_COUNT,
  "files_with_exclusions": $FILE_COUNT
}
EOF
        ;;

    markdown)
        cat <<EOF
## Coverage Audit Report

**Date:** $AUDIT_DATE

### Coverage Summary

| Metric | Value |
|--------|-------|
| Line Coverage (with exclusions) | ${COVERAGE_WITH}% |
| Line Coverage (without exclusions) | ${COVERAGE_WITHOUT}% |
| Gap from exclusions | ${GAP}% |
| Coverage exclusion annotations | ${EXCLUSION_COUNT} |
| Threshold | ${THRESHOLD}% |

### Status

EOF
        if [ "$EXCEEDS_THRESHOLD" = true ]; then
            echo "**WARNING:** Gap exceeds threshold of ${THRESHOLD}%"
            echo ""
        else
            echo "Gap is within acceptable threshold."
            echo ""
        fi

        cat <<EOF
### Raw Output

**With exclusions:**
\`\`\`
$TOTAL_WITH
\`\`\`

**Without exclusions:**
\`\`\`
$TOTAL_WITHOUT
\`\`\`

### Files with Exclusions

\`\`\`
EOF
        if [ -n "$EXCLUSION_FILES" ]; then
            echo "$EXCLUSION_FILES"
        else
            echo "No exclusions found"
        fi
        echo '```'
        ;;

    text)
        echo ""
        echo -e "${BOLD}Coverage Audit Report${NC}"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo ""
        printf "  %-35s %s\n" "Coverage (with exclusions):" "${COVERAGE_WITH}%"
        printf "  %-35s %s\n" "Coverage (without exclusions):" "${COVERAGE_WITHOUT}%"
        printf "  %-35s %s\n" "Gap from exclusions:" "${GAP}%"
        printf "  %-35s %s\n" "Exclusion annotations:" "${EXCLUSION_COUNT}"
        printf "  %-35s %s\n" "Threshold:" "${THRESHOLD}%"
        echo ""

        if [ "$EXCEEDS_THRESHOLD" = true ]; then
            echo -e "  ${RED}${BOLD}WARNING:${NC} Gap exceeds threshold of ${THRESHOLD}%"
            echo ""
            echo "  Consider reviewing coverage exclusions to ensure they are"
            echo "  only used for truly untestable code paths."
        else
            echo -e "  ${GREEN}${BOLD}OK:${NC} Gap is within acceptable threshold."
        fi

        echo ""
        echo -e "${BOLD}Files with exclusions:${NC}"
        if [ -n "$EXCLUSION_FILES" ]; then
            echo "$EXCLUSION_FILES" | sed 's/^/  /'
        else
            echo "  (none)"
        fi
        echo ""
        ;;
esac

# Exit with non-zero if threshold exceeded (useful for CI)
if [ "$EXCEEDS_THRESHOLD" = true ]; then
    exit 1
fi
