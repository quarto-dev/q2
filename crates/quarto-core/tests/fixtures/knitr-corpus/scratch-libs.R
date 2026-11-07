#!/usr/bin/env Rscript
# Build one scratch R library per pinned knitr version (KNR-1, spike S1).
#
#   Rscript scratch-libs.R <lib-root> [<knitr-version> ...]
#
# Needs network access (CRAN and its archive). Creates
#   <lib-root>/knitr-<version>/   holding knitr, evaluate and xfun at the pins below.
# Other dependencies (highr, yaml, ...) come from the default library, so a
# column of results is "knitr X + evaluate Y + xfun Z + whatever the machine has".
# Callers select a library by putting it first on R_LIBS (build the value with
# std::env::join_paths from Rust, or file.path/.Platform$path.sep in R).
#
# Pins: evaluate and xfun are the versions current at each knitr release date,
# read from the CRAN archive (dates recorded in
# claude-notes/plans/2026-10-30-knr-01-capture-spike-and-harness.md, S1).
# knitr 1.50 is deliberately pinned to the evaluate/xfun that q2 developers have
# installed (evaluate 1.0.4, xfun 0.52), not to those current on its release date.
# "current" resolves against CRAN main, "archive" against the CRAN archive.

pins <- list(
  "1.44" = c(knitr = "1.44", evaluate = "0.21",  xfun = "0.40"),
  "1.50" = c(knitr = "1.50", evaluate = "1.0.4", xfun = "0.52"),
  "1.52" = c(knitr = "1.52", evaluate = "1.0.5", xfun = "0.62")
)

args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) stop("usage: Rscript scratch-libs.R <lib-root> [<knitr-version> ...]")
lib_root <- normalizePath(args[[1]], mustWork = FALSE)
want <- if (length(args) > 1) args[-1] else names(pins)
bad <- setdiff(want, names(pins))
if (length(bad)) stop("no pin for knitr ", paste(bad, collapse = ", "))
dir.create(lib_root, recursive = TRUE, showWarnings = FALSE)

cran <- "https://cran.r-project.org/src/contrib"

# Tarballs go into their own fresh download directory, away from this script.
fetch <- function(pkg, ver, dest_dir) {
  file <- sprintf("%s_%s.tar.gz", pkg, ver)
  dest <- file.path(dest_dir, file)
  for (url in c(file.path(cran, file), file.path(cran, "Archive", pkg, file))) {
    ok <- tryCatch(
      suppressWarnings(utils::download.file(url, dest, quiet = TRUE, mode = "wb")) == 0,
      error = function(e) FALSE
    )
    if (ok && file.exists(dest) && file.size(dest) > 0) return(dest)
  }
  stop("could not download ", file)
}

for (kv in want) {
  pin <- pins[[kv]]
  lib <- file.path(lib_root, paste0("knitr-", kv))
  dir.create(lib, recursive = TRUE, showWarnings = FALSE)
  dl <- tempfile("knr-dl-"); dir.create(dl)
  # Install order matters: knitr checks the xfun/evaluate it can see at install time.
  .libPaths(c(lib, .libPaths()))
  for (pkg in c("xfun", "evaluate", "knitr")) {
    have <- tryCatch(as.character(utils::packageVersion(pkg, lib.loc = lib)), error = function(e) NA)
    if (identical(have, unname(pin[[pkg]]))) next
    tarball <- fetch(pkg, pin[[pkg]], dl)
    utils::install.packages(tarball, repos = NULL, type = "source", lib = lib,
                            INSTALL_opts = "--no-docs")
  }
  got <- vapply(c("knitr", "evaluate", "xfun"), function(p) {
    as.character(utils::packageVersion(p, lib.loc = lib))
  }, "")
  cat(sprintf("knitr-%s: %s\n", kv, paste(names(got), got, sep = "=", collapse = " ")))
  unlink(dl, recursive = TRUE)
}
