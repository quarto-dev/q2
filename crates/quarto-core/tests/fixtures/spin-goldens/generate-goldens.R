#!/usr/bin/env Rscript
# Regenerate the committed knitr golden corpus for Plan 7b's `spin` content
# processor. Pinned oracle: knitr 1.50 / R 4.3.2 (see the plan's Phase 0
# research note, claude-notes/research/2026-09-24-plan7b-phase0-spike.md).
#
# Run from the repo root:
#   Rscript crates/quarto-core/tests/fixtures/spin-goldens/generate-goldens.R
#
# Regenerate whenever bumping the pinned knitr version — diff the outputs
# and update this comment's pin if `spin()`'s qmd-branch output changes.

installed <- as.character(packageVersion("knitr"))
cat("Using knitr", installed, "\n")

dir <- "crates/quarto-core/tests/fixtures/spin-goldens"
stopifnot(dir.exists(dir))

fixtures <- list.files(dir, pattern = "\\.R$", full.names = TRUE)
fixtures <- fixtures[basename(fixtures) != "generate-goldens.R"]

for (f in fixtures) {
  out <- knitr::spin(f, knit = FALSE, report = FALSE, format = "qmd")
  cat("wrote", out, "\n")
}
