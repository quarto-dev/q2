#!/bin/bash
# quick-error-audit.sh
# Quick audit of error code consistency between catalog and source code
#
# This script checks for:
# - Error codes used in source but missing from catalog
# - Error codes in catalog but never used in source
# - Overall consistency statistics

set -e

# Get repository root (assuming script is in scripts/ directory)
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "=== Error Code Audit ==="
echo "Repository: $REPO_ROOT"
echo

# Extract catalog codes
echo "📚 Extracting catalog codes..."
if [ ! -f "crates/quarto-error-reporting/error_catalog.json" ]; then
  echo "❌ Error: Cannot find error_catalog.json"
  exit 1
fi

jq -r 'keys[]' crates/quarto-error-reporting/error_catalog.json | sort > /tmp/catalog-codes.txt
catalog_count=$(wc -l < /tmp/catalog-codes.txt | tr -d ' ')
echo "   Found $catalog_count codes in catalog"

# Search source codes
echo "🔍 Searching source code..."
rg 'Q-\d+-\d+' \
  --type rust --type json --type markdown \
  --glob '!target/' \
  --glob '!external-sources/' \
  --glob '!external-sites/' \
  --no-filename --no-line-number --only-matching | \
  grep -oE 'Q-[0-9]+-[0-9]+' | \
  sort -u > /tmp/source-codes.txt
source_count=$(wc -l < /tmp/source-codes.txt | tr -d ' ')
echo "   Found $source_count unique codes in source"

# Compare and analyze
echo

# Missing from catalog (HIGH PRIORITY)
comm -13 /tmp/catalog-codes.txt /tmp/source-codes.txt > /tmp/missing.txt
missing_count=$(wc -l < /tmp/missing.txt | tr -d ' ')

# Orphaned in catalog (MEDIUM PRIORITY)
comm -23 /tmp/catalog-codes.txt /tmp/source-codes.txt > /tmp/orphaned.txt
orphaned_count=$(wc -l < /tmp/orphaned.txt | tr -d ' ')

# Consistent (GOOD)
comm -12 /tmp/catalog-codes.txt /tmp/source-codes.txt > /tmp/consistent.txt
consistent_count=$(wc -l < /tmp/consistent.txt | tr -d ' ')

# Print summary
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Summary:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Codes in catalog:  $catalog_count"
echo "  Codes in source:   $source_count"
echo "  Consistent:        $consistent_count ✅"
echo "  Missing (catalog): $missing_count $([ $missing_count -gt 0 ] && echo '❌' || echo '✅')"
echo "  Orphaned (unused): $orphaned_count $([ $orphaned_count -gt 0 ] && echo '⚠️' || echo '✅')"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Show issues if any
has_issues=false

if [ $missing_count -gt 0 ]; then
  has_issues=true
  echo
  echo "❌ MISSING FROM CATALOG (HIGH PRIORITY):"
  echo "   These codes are used in source but have no catalog entry"
  echo
  while IFS= read -r code; do
    echo "   • $code"
    # Show first occurrence
    location=$(rg "$code" --type rust --type json -l --glob '!target/' | head -1)
    if [ -n "$location" ]; then
      echo "     ↳ $location"
    fi
  done < /tmp/missing.txt
fi

if [ $orphaned_count -gt 0 ]; then
  has_issues=true
  echo
  echo "⚠️  ORPHANED IN CATALOG (MEDIUM PRIORITY):"
  echo "   These codes are in catalog but never used in source"
  echo
  cat /tmp/orphaned.txt | sed 's/^/   • /'
fi

if [ "$has_issues" = false ]; then
  echo
  echo "✅ No issues found! All error codes are consistent."
fi

# Detailed breakdown by subsystem
echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Breakdown by Subsystem:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

for subsystem in 0 1 2 3; do
  case $subsystem in
    0) name="Internal (Q-0-*)" ;;
    1) name="YAML (Q-1-*)" ;;
    2) name="Markdown (Q-2-*)" ;;
    3) name="Writer (Q-3-*)" ;;
  esac

  catalog_sub=$(grep "^Q-$subsystem-" /tmp/catalog-codes.txt 2>/dev/null | wc -l | tr -d ' ')
  source_sub=$(grep "^Q-$subsystem-" /tmp/source-codes.txt 2>/dev/null | wc -l | tr -d ' ')

  echo "  $name"
  echo "    Catalog: $catalog_sub  Source: $source_sub"
done

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Temporary files:"
echo "  /tmp/catalog-codes.txt  - All codes in catalog"
echo "  /tmp/source-codes.txt   - All codes in source"
echo "  /tmp/consistent.txt     - Codes in both"
echo "  /tmp/missing.txt        - Missing from catalog"
echo "  /tmp/orphaned.txt       - Unused in catalog"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Exit with error code if issues found
if [ "$has_issues" = true ]; then
  echo
  echo "💡 See claude-notes/workflows/2025-11-23-error-code-audit-workflow.md"
  echo "   for detailed instructions on resolving these issues."
  exit 1
else
  exit 0
fi
