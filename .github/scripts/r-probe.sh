#!/usr/bin/env bash
# Diagnostic for bd-9kbjjeb6: which R runs, and where its library comes from.
# Usage: r-probe.sh <label>. Always exits 0.
set +e
Q='cat("R.home=", R.home(), "\n"); print(.libPaths());
print(Sys.getenv(c("R_LIBS_USER","R_LIBS","R_LIBS_SITE","R_ENVIRON","R_ENVIRON_USER","R_PROFILE","R_PROFILE_USER","RVENV","R_DEFAULT_PACKAGES")));
cat("rmarkdown:", tryCatch(find.package("rmarkdown"), error = function(e) conditionMessage(e)), "\n")'
run() { echo "--- $*"; "$@"; echo "status=$?"; }
echo "=== $1 (cwd=$PWD)"
echo "PATH=$PATH"
type -a Rscript
env | grep -E '^(R_|RVENV|QUARTO_R|QUARTO_KNITR)' | sort
ls -la ~/.Renviron ~/.Rprofile 2>&1
ls -la "$GITHUB_WORKSPACE/.rvenv/lib" 2>&1 | head -5
for rs in Rscript "$GITHUB_WORKSPACE/.rvenv/bin/Rscript"; do
  for flag in "" --no-environ --no-site-file --no-init-file --vanilla; do
    run "$rs" $flag -e "$Q"
  done
done
run Rscript -e 'for (f in file.path(R.home("etc"), c("Renviron.site", "Rprofile.site"))) { cat("==", f, "\n"); if (file.exists(f)) writeLines(readLines(f)) }'
run Rscript -e 'x <- readLines(file.path(R.home("library"), "base", "R", "Rprofile")); i <- grep("^## rig", x); print(x[i])'
exit 0
