#!/usr/bin/env Rscript
# KNR-1 spike S1: run every s1/ fixture under every mechanism and knitr version.
#
#   [S1_FIXTURES=<dir of .Rmd fixtures, default: this directory>]
#   [S1_MECHS=off,evalhook,engcopy,sew,chunkhook] Rscript run-matrix.R <lib-root> <out-root> <rmd-resources-dir> [version ...]
#
# <lib-root> is what scratch-libs.R built. Writes
#   <out-root>/<version>/<fixture-stem>/<mechanism>/{capture.runN.json,md.runN.md}
# The cache fixtures (14_*) are run twice, each run a fresh R process.

args <- commandArgs(trailingOnly = TRUE)
lib_root <- normalizePath(args[[1]])
out_root <- args[[2]]
rmd_dir <- normalizePath(args[[3]])
versions <- if (length(args) > 3) args[-(1:3)] else c("1.44", "1.50", "1.52")

here <- dirname(normalizePath(sub("^--file=", "", grep("^--file=", commandArgs(), value = TRUE)[[1]])))
harness <- file.path(here, "harness.R")
fixtures <- sort(list.files(Sys.getenv("S1_FIXTURES", here), pattern = "\\.Rmd$", full.names = TRUE))
mechs <- strsplit(Sys.getenv("S1_MECHS", "off,evalhook,engcopy,sew,chunkhook"), ",")[[1]]

jobs <- expand.grid(fixture = fixtures, mech = mechs, version = versions, stringsAsFactors = FALSE)

run_job <- function(i) {
  j <- jobs[i, ]
  stem <- sub("\\.Rmd$", "", basename(j$fixture))
  out <- file.path(out_root, j$version, stem, j$mech)
  runs <- if (grepl("^14_", stem)) 1:2 else 1L
  rlibs <- paste(c(file.path(lib_root, paste0("knitr-", j$version)), Sys.getenv("R_LIBS")),
                 collapse = .Platform$path.sep)
  for (r in runs) {
    log <- system2(file.path(R.home("bin"), "Rscript"),
                   c(shQuote(harness), shQuote(j$fixture), j$mech, shQuote(out), shQuote(rmd_dir), r),
                   env = paste0("R_LIBS=", rlibs), stdout = TRUE, stderr = TRUE)
    writeLines(log, file.path(dirname(out), paste0(j$mech, ".run", r, ".log")))
  }
  invisible(NULL)
}

cores <- if (.Platform$OS.type == "unix") 4L else 1L
invisible(parallel::mclapply(seq_len(nrow(jobs)), run_job, mc.cores = cores))
cat("done:", nrow(jobs), "jobs\n")
