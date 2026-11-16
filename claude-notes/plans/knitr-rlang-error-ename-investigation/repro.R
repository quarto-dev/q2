# bd-54odm3rz: why capture.R reports ename "rlang_error" for stop("boom").
#
# knitr >= 1.51 (yihui/knitr#2388) adds `rlang_entrace_handler` to the
# calling handlers it passes to evaluate (knitr:::knit_handlers) whenever
# rlang is available. rlang::entrace() re-signals every error as a fresh
# condition of class c("rlang_error", "error", "condition"), keeping the
# original under `$error`. capture.R's `ename = class(p)[[1]]` then sees
# "rlang_error". Run with: Rscript repro.R

cat("knitr", as.character(packageVersion("knitr")),
    "evaluate", as.character(packageVersion("evaluate")),
    "rlang", as.character(packageVersion("rlang")), "\n")

print(knitr:::rlang_entrace_handler)

entraced <- function(expr) {
  tryCatch(
    withCallingHandlers(expr, error = function(e) rlang::entrace(e)),
    error = function(e) e
  )
}

r <- entraced(stop("boom"))
cat("base stop():       ", class(r), "\n")
cat("  original class:  ", class(r$error), "\n")
cat("  as.character():  ", deparse(as.character(r)), "\n")

custom <- structure(
  class = c("myError", "error", "condition"),
  list(message = "x", call = NULL)
)
r2 <- entraced(stop(custom))
cat("custom condition:  ", class(r2), "\n")
cat("  original class:  ", class(r2$error), "\n")
