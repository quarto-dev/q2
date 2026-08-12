#!/bin/bash
# indent x context sweep for '[' at line start after a paragraph line
cd /Users/cscheid/rooms/room-2/q2/crates/tree-sitter-qmd/tree-sitter-markdown
probe() {
  local name="$1"; local body="$2"
  printf '%b' "$body" > /tmp/q2probe.qmd
  echo "### $name"
  printf '%b' "$body" | sed 's/^/    | /'
  echo "  --->"
  tree-sitter parse /tmp/q2probe.qmd 2>&1 | grep -oE '\(([a-z_]+)' | tr -d '(' | tr '\n' ' '
  echo; echo
}
probe "A0 indent0 adjacent defs" '[^a]: one.\n[^b]: two.\n'
probe "A1 indent1 second def" '[^a]: one.\n [^b]: two.\n'
probe "A3 indent3 second def" '[^a]: one.\n   [^b]: two.\n'
probe "A4 indent4 second def" '[^a]: one.\n    [^b]: two.\n'
probe "B  paragraph then def" 'hello there.\n[^b]: two.\n'
probe "C  paragraph then bracket-not-def" 'hello there.\n[not a footnote] in prose.\n'
probe "D  paragraph then bare bracket ref" 'hello there.\n[^b] is a ref.\n'
probe "E  blockquote adjacent defs" '> [^a]: one.\n> [^b]: two.\n'
probe "F  in list item" '- item text\n  [^b]: two.\n'
probe "G  link-style ref def line" 'hello there.\n[ref]: http://example.com\n'
