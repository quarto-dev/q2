#!/usr/bin/env bash
# bd-8svy3p89 repro: project metadata that crosses the engine round-trip.
# Usage: subdir-repro.sh <path-to-q2>   (needs python + jupyter for code.qmd)
set -euo pipefail
Q2=${1:?path to q2 binary}
dir=$(mktemp -d)
cd "$dir"
mkdir -p docs _partials
cat > _quarto.yml <<'YML'
project:
  type: website
website:
  title: "My_site_name"
format:
  html:
    css: styles.css
    include-in-header: _partials/_head.html
YML
echo '<meta name="probe-head" content="yes">' > _partials/_head.html
echo '.probe-css{}' > styles.css
printf -- '---\ntitle: Plain\n---\n\nNo code.\n' > docs/plain.qmd
printf -- '---\ntitle: Code\n---\n\n```{python}\nprint(1)\n```\n' > docs/code.qmd
"$Q2" render
for f in plain code; do
  echo "== docs/$f.html"
  grep -o 'href="[^"]*styles.css"\|probe-head' "_site/docs/$f.html" || true
done
echo "(project in $dir)"
