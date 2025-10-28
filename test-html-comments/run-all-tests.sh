#!/bin/bash

# Test runner for HTML comment tests
# Run from the repository root directory

echo "HTML Comment Test Suite"
echo "======================="
echo

failed=0
passed=0
total=0

for file in test-html-comments/*.qmd; do
    if [ -f "$file" ]; then
        total=$((total + 1))
        basename=$(basename "$file")
        printf "Testing %-40s ... " "$basename"

        # Run the parser and capture output
        output=$(cargo run --bin quarto-markdown-pandoc -- -i "$file" 2>&1)
        exit_code=$?

        if [ $exit_code -eq 0 ]; then
            echo "✓ PASS"
            passed=$((passed + 1))
        else
            echo "✗ FAIL"
            failed=$((failed + 1))
            echo "  Error output:"
            echo "$output" | head -20 | sed 's/^/    /'
            echo
        fi
    fi
done

echo
echo "======================="
echo "Results: $passed passed, $failed failed, $total total"
echo

if [ $failed -eq 0 ]; then
    echo "All tests passed! ✓"
    exit 0
else
    echo "Some tests failed."
    exit 1
fi
