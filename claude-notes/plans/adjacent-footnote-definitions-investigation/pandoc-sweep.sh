#!/bin/bash
p() {
  echo "### $1"
  printf '%b' "$2" | sed 's/^/    | /'
  echo "  ---> pandoc -f markdown -t native:"
  printf '%b' "$2" | pandoc -f markdown -t native 2>&1 | sed 's/^/      /'
  echo
}
p "A0 indent0 adjacent defs" '[^a]: one.\n[^b]: two.\n'
p "A1 indent1 second def" '[^a]: one.\n [^b]: two.\n'
p "A3 indent3 second def" '[^a]: one.\n   [^b]: two.\n'
p "A4 indent4 second def" '[^a]: one.\n    [^b]: two.\n'
p "B  paragraph then def" 'hello there.\n[^b]: two.\n'
p "D  paragraph then bare ref" 'hello there.\n[^b] is a ref.\n'
p "E  blockquote adjacent defs" '> [^a]: one.\n> [^b]: two.\n'
p "F  in list item" '- item text\n  [^b]: two.\n'
