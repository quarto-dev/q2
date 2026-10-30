#!/usr/bin/env Rscript
# KNR-1 spike S1 harness: run one fixture under one capture mechanism and one
# knitr version (chosen by the caller through R_LIBS) and write the result as
# JSON. Spike code: it observes what each mechanism sees; it ships nothing.
#
#   Rscript harness.R <fixture.Rmd> <mechanism> <out-dir> <rmd-resources-dir> [run]
#
# mechanism:
#   off        control, no observer
#   evalhook   (1) wrap knit_hooks$get('evaluate'): pieces before the filters
#   engcopy    (2) replacement copy of knitr:::eng_r (here: the original body with
#              one capture call injected after the `res = ... evaluate(...)` step)
#   sew        (3) wrap q2's replacement of knitr:::sew: pieces after the filters
#   chunkhook  observer on knit_hooks$get('chunk'): final options and whether it runs
# run: 1 wipes the work directory; 2 reuses it (cache hits). One run per process,
# like q2 (a render is a fresh R process).
#
# <rmd-resources-dir> is crates/quarto-core/src/engine/knitr/resources/rmd. Its
# patch.R, execute.R and hooks.R are sourced unchanged, in rmd.R's order, with a synthetic `format` (HTML
# target, default execute options) standing in for the one Rust passes.

args <- commandArgs(trailingOnly = TRUE)
fixture <- normalizePath(args[[1]])
mech <- args[[2]]
outdir <- args[[3]]
rmd_dir <- normalizePath(args[[4]])
run <- if (length(args) >= 5) as.integer(args[[5]]) else 1L

dir.create(outdir, recursive = TRUE, showWarnings = FALSE)
outdir <- normalizePath(outdir)
work <- file.path(outdir, "work")
if (run == 1L) {
  unlink(work, recursive = TRUE)
  dir.create(work)
  file.copy(fixture, file.path(work, "doc.Rmd"))
  # child documents sit beside the fixture and are named child*.Rmd
  file.copy(list.files(dirname(fixture), pattern = "^child.*[.]Rmd$", full.names = TRUE), work)
}
setwd(work)

suppressPackageStartupMessages(library(knitr))
.s1_versions <- vapply(c("knitr", "evaluate", "xfun"), function(p) {
  as.character(utils::packageVersion(p))
}, "")

# ---- q2's R layer, unchanged -------------------------------------------------
source(file.path(rmd_dir, "patch.R"))
source(file.path(rmd_dir, "execute.R"))  # helpers only; hooks.R calls them
source(file.path(rmd_dir, "hooks.R"))

.s1_format <- list(
  execute = list(
    `fig-width` = 7, `fig-height` = 5, `fig-format` = "png", `fig-dpi` = 96,
    eval = TRUE, error = FALSE, echo = TRUE, warning = TRUE, include = TRUE,
    `df-print` = "default"
  ),
  pandoc = list(to = Sys.getenv("S1_TO", "html"), from = "markdown"),
  render = list(),
  metadata = list()
)
.s1_format$execute$`df-print` <- Sys.getenv("S1_DFPRINT", "default")
# S1_RENDER=1: go through rmarkdown::render like execute.R does, instead of knit()
.s1_render <- nzchar(Sys.getenv("S1_RENDER"))
.s1_ko <- NULL
if (.s1_render) {
  .s1_ko <- knitr_options(.s1_format, rmd_dir, c("ojs", "mermaid", "dot"))  # q2's own
} else {
  # Mirrors knitr_options() in execute.R (the part that does not need rmarkdown).
  knitr::render_markdown()
  knitr::opts_knit$set(
    quarto.version = 1, rmarkdown.pandoc.from = "markdown",
    rmarkdown.pandoc.to = .s1_format$pandoc$to, rmarkdown.version = 3,
    rmarkdown.runtime = "static"
  )
  knitr::opts_chunk$set(
    fig.width = 7, fig.height = 5, dev = "png", dpi = 96, eval = TRUE, error = FALSE,
    echo = TRUE, fenced.echo = FALSE, warning = TRUE, message = TRUE, include = TRUE,
    comment = NA, ft.shadow = FALSE
  )
  .s1_hooks <- knitr_hooks(.s1_format, rmd_dir, c("ojs", "mermaid", "dot"))
  knitr::knit_hooks$set(.s1_hooks$knit)
  knitr::opts_hooks$set(.s1_hooks$opts)
}
.s1_hook_get <- function(n) {
  if (.s1_render && !is.null(.s1_ko$knit_hooks[[n]])) .s1_ko$knit_hooks[[n]] else knitr::knit_hooks$get(n)
}
.s1_hook_set <- function(n, f) {
  if (.s1_render) .s1_ko$knit_hooks[[n]] <<- f else knitr::knit_hooks$set(setNames(list(f), n))
}

# ---- recording -----------------------------------------------------------------
.s1 <- new.env()
.s1$events <- list()
.s1$metas <- list()
.s1$results <- list()

.s1_sanitize <- function(v) {
  if (is.null(v)) return(NULL)
  if (is.language(v)) return(list(`__language__` = paste(deparse(v), collapse = " ")))
  if (is.function(v)) return("<function>")
  if (is.environment(v)) return("<environment>")
  if (is.atomic(v) && length(v) <= 8) return(as.list(v))
  paste0("<", class(v)[[1]], " of length ", length(v), ">")
}

.s1_opts <- function(o, keys = c(
  "label", "echo", "eval", "include", "results", "warning", "message", "error",
  "fig.keep", "fig.show", "cache", "collapse", "fig.cap", "fig.num", "fig.cur"
)) {
  out <- lapply(o[intersect(keys, names(o))], .s1_sanitize)
  # keep NULL-valued keys visible
  out
}

.s1_summ <- function(p) {
  cls <- class(p)
  if (inherits(p, "source")) {
    list(kind = "source", text = paste(p$src, collapse = ""))
  } else if (inherits(p, "knit_asis")) {
    list(kind = "knit_asis", classes = cls, n_meta = length(attr(p, "knit_meta")),
         text = substr(paste(p, collapse = ""), 1, 120))
  } else if (inherits(p, "knit_image_paths")) {
    list(kind = "knit_image_paths", paths = as.character(p))
  } else if (is.character(p)) {
    list(kind = "character", text = paste(p, collapse = ""))
  } else if (inherits(p, "error")) {
    list(kind = "error", text = conditionMessage(p))
  } else if (inherits(p, "warning")) {
    list(kind = "warning", text = conditionMessage(p),
         call = if (is.null(conditionCall(p))) NULL else deparse(conditionCall(p))[[1]])
  } else if (inherits(p, "message")) {
    list(kind = "message", text = conditionMessage(p))
  } else if (inherits(p, "recordedplot")) {
    list(kind = "recordedplot", n_calls = length(p[[1]]))
  } else {
    list(kind = "other", classes = cls)
  }
}

.s1_rec <- function(point, pieces, opts, extra = list()) {
  .s1$events[[length(.s1$events) + 1L]] <- c(list(
    point = point,
    label = opts$label,
    options = .s1_opts(opts),
    pieces = lapply(pieces, .s1_summ)
  ), extra)
}

# S4: copy of what execute.R does for `execute: cache: false` (execute.R:394-399)
if (nzchar(Sys.getenv("S1_NOCACHE"))) {
  knitr::opts_hooks$set(cache = function(options) {
    options$cache <- FALSE
    options
  })
}

# execute.R defines this inside execute(); fixtures use the preprocessed form
assign(".QuartoInlineRender", function(v) {
  if (is.null(v)) {
    "NULL"
  } else if (inherits(v, "AsIs")) {
    v
  } else if (is.character(v)) {
    gsub(pattern = "(\\[|\\]|[`*_{}()>#+-.!])", x = v, replacement = "\\\\\\1")
  } else {
    v
  }
}, envir = globalenv())

# ---- mechanisms ----------------------------------------------------------------
if (mech == "evalhook") {
  orig <- .s1_hook_get("evaluate")
  .s1_hook_set("evaluate", function(code, ...) {
    res <- orig(code, ...)
    o <- knitr::opts_current$get()
    # capture-time plot selection with knitr's own function (fig.show left alone)
    keep <- o$fig.keep; keep_idx <- NULL
    if (is.logical(keep)) keep <- which(keep)
    if (is.numeric(keep)) { keep_idx <- keep; keep <- "index" }
    sel <- tryCatch(knitr:::rearrange_figs(res, keep, keep_idx, "asis"), error = function(e) NULL)
    np <- function(x) sum(vapply(x, evaluate::is.recordedplot, logical(1)))
    .s1$metas[[o$label]] <- unlist(lapply(res, function(p) if (inherits(p, "knit_asis")) attr(p, "knit_meta")), recursive = FALSE)
    .s1$results[[o$label]] <- o$results
    .s1_rec("evaluate-hook", res, o, list(
      code = paste(code, collapse = "\n"),
      plots_raw = np(res), plots_selected = if (is.null(sel)) NA else np(sel)
    ))
    res
  })
} else if (mech == "inline") {
  # S9: observe knitr's inline hooks (execute.R defines .QuartoInlineRender inside execute())
  .s1$inl <- list(); .s1$cur <- NULL
  ev_prev <- knitr::hooks_markdown()[["evaluate.inline"]] %||% .s1_hook_get("evaluate.inline")
  in_prev <- knitr::hooks_markdown()$inline  # what render_markdown() installs during the run
  .s1_hook_set("evaluate.inline", function(code, envir = knit_global()) {
    v <- ev_prev(code, envir)
    .s1$ev <- c(.s1$ev, list(list(code = code, is_null = is.null(v))))
    .s1$cur <- list(seq = length(.s1$inl) + 1L, code = code, value_class = class(v),
                    value_is_null = is.null(v), value_text = substr(paste(as.character(v), collapse = "|"), 1, 80))
    v
  })
  .s1_hook_set("inline", function(x) {
    out <- in_prev(x)
    cur <- .s1$cur; if (is.null(cur)) cur <- list(seq = length(.s1$inl) + 1L, code = NA)
    cur$x_class <- class(x); cur$formatted <- paste(as.character(out), collapse = "|")
    .s1$inl[[length(.s1$inl) + 1L]] <- cur; .s1$cur <- NULL
    out
  })
} else if (mech == "markers") {
  # S7: the chunk hook returns a marker instead of the formatted chunk; labels
  # starting with fb_ are fallback chunks and keep their real markdown
  chunk_prev <- .s1_hook_get("chunk")
  .s1$n <- 0L
  .s1_hook_set("chunk", function(x, options) {
    real <- chunk_prev(x, options)
    .s1$n <- .s1$n + 1L
    fallback <- grepl("^fb_", options$label)
    .s1$events[[length(.s1$events) + 1L]] <- list(
      point = "marker", label = options$label, unit = .s1$n, fallback = fallback,
      indent = if (is.null(options$indent)) NA else options$indent, pieces = list())
    if (fallback) real else paste0(if (is.null(options$indent)) "" else options$indent, sprintf("<!-- knr-unit:%d:%s -->\n", .s1$n, options$label))
  })
} else if (mech == "hookdetect") {
  # S5: can an overridden knit hook be detected from the chunk hook?
  .s1$base_hooks <- if (.s1_render) .s1_ko$knit_hooks else as.list(knitr::knit_hooks$get())
  chunk_prev <- .s1_hook_get("chunk")
  .s1_hook_set("chunk", function(x, options) {
    changed <- Filter(function(n) {
      n != "chunk" && !identical(knitr::knit_hooks$get(n), .s1$base_hooks[[n]])
    }, names(.s1$base_hooks))
    .s1$events[[length(.s1$events) + 1L]] <- list(
      point = "chunk-hook", label = options$label, options = .s1_opts(options),
      pieces = list(), changed_hooks = as.list(changed))
    chunk_prev(x, options)
  })
} else if (mech == "evalforce") {
  # NOT a candidate mechanism: tests what forcing keep_warning/keep_message does
  orig <- .s1_hook_get("evaluate")
  .s1_hook_set("evaluate", function(code, ...) {
    a <- list(...)
    a$keep_warning <- TRUE
    a$keep_message <- TRUE
    res <- do.call(orig, c(list(code), a))
    .s1_rec("evaluate-hook-forced", res, knitr::opts_current$get(),
            list(code = paste(code, collapse = "\n")))
    res
  })
} else if (mech == "engcopy") {
  .s1_rec_engine <- function(res, options, code) {
    .s1_rec("eng_r-copy", res, options, list(code = paste(code, collapse = "\n")))
    invisible(NULL)
  }
  eng <- knitr:::eng_r
  b <- body(eng)
  idx <- NA_integer_
  for (i in seq_along(b)) {
    e <- b[[i]]
    if (is.call(e) && identical(e[[1]], as.name("=")) && identical(e[[2]], as.name("res")) &&
        grepl("evaluate(", paste(deparse(e), collapse = " "), fixed = TRUE)) {
      idx <- i
      break
    }
  }
  if (is.na(idx)) stop("S1 harness: injection anchor not found in knitr:::eng_r")
  body(eng) <- as.call(append(as.list(b), list(quote(.s1_rec_engine(res, options, code))), after = idx))
  assignInNamespace("eng_r", eng, ns = "knitr")
  .s1$eng_r_lines <- length(deparse(knitr:::eng_r))
} else if (mech == "sew") {
  sew_prev <- get("sew", asNamespace("knitr"))
  sew_wrap <- function(x, options = list(), ...) {
    if (!missing(options) && inherits(x, "list") && length(options) > 0L &&
        !is.null(options$label)) {
      .s1_rec("sew", x, options)
    }
    if (missing(options)) sew_prev(x, ...) else sew_prev(x, options, ...)
  }
  assignInNamespace("sew", sew_wrap, ns = "knitr")
} else if (mech == "chunkhook") {
  chunk_prev <- .s1_hook_get("chunk")
  .s1_hook_set("chunk", function(x, options) {
    .s1$events[[length(.s1$events) + 1L]] <- list(
      point = "chunk-hook", label = options$label, options = .s1_opts(options),
      all_options = lapply(options, function(v) list(
        type = if (is.null(v)) "NULL" else paste(class(v), collapse = "/"),
        value = if (is.function(v) || is.environment(v)) NULL else .s1_sanitize(v))),
      pieces = list(), output_chars = nchar(paste(x, collapse = ""))
    )
    chunk_prev(x, options)
  })
} else if (mech != "off") {
  stop("unknown mechanism: ", mech)
}

# ---- run -----------------------------------------------------------------------
set.seed(1)
.s1$knit_meta <- NULL
status <- tryCatch({
  if (.s1_render) {
    df_print <- .s1_format$execute$`df-print`
    is_html <- .s1_format$pandoc$to %in% c("html", "html4", "html5")
    if (df_print == "paged" && !is_html) df_print <- "kable"  # as execute.R
    fmt <- rmarkdown::output_format(
      knitr = .s1_ko,
      pandoc = rmarkdown::pandoc_options(to = .s1_format$pandoc$to, from = "markdown",
                                         args = c("--to", .s1_format$pandoc$to)),
      keep_md = FALSE, clean_supporting = TRUE, df_print = df_print
    )
    out <- rmarkdown::render("doc.Rmd", output_format = fmt, knit_root_dir = getwd(),
                             run_pandoc = FALSE, envir = globalenv(), quiet = TRUE)
    file.copy(out, "out.md", overwrite = TRUE)
    file.copy(out, "out.preserve.md", overwrite = TRUE)
    pres <- tryCatch(extract_preserve_chunks(file.path(getwd(), "out.preserve.md"), .s1_format),
                     error = function(e) paste("error:", conditionMessage(e)))
    .s1$preserved <- if (is.null(pres)) "NULL" else if (is.character(pres)) length(pres) else "other"
    .s1$after_preserve_markers <- sum(grepl("knr-unit", readLines("out.preserve.md", warn = FALSE)))
    km <- attr(out, "knit_meta")
    if (length(.s1$metas)) {
      kept <- unlist(.s1$metas[names(.s1$results)[vapply(.s1$results, function(r) !identical(r, "hide"), TRUE)]], recursive = FALSE)
      rt <- lapply(kept, function(m) jsonlite::unserializeJSON(jsonlite::serializeJSON(m)))
      nm <- function(x) vapply(x, function(m) paste(m$name, m$version), "")
      first_diff <- NULL
      for (i in seq_along(kept)) if (!identical(unname(kept)[[i]], unname(km)[[i]])) {
        first_diff <- paste(nm(kept)[i], "::", paste(utils::capture.output(print(all.equal(unname(kept)[[i]], unname(km)[[i]]))), collapse = " | ")); break }
      first_rt_diff <- NULL
      for (i in seq_along(kept)) if (!identical(rt[[i]], unname(kept)[[i]])) {
        first_rt_diff <- paste(nm(kept)[i], "::", paste(utils::capture.output(print(all.equal(rt[[i]], unname(kept)[[i]]))), collapse = " | ")); break }
      .s1$meta_check <- list(
        same_name_version_sequence = identical(unname(nm(kept)), unname(nm(km))),
        elementwise_identical_live = length(kept) == length(km) && all(mapply(identical, unname(kept), unname(km))),
        elementwise_identical_roundtrip = length(kept) == length(rt) && all(mapply(identical, rt, unname(kept))),
        all_equal_live = isTRUE(all.equal(unname(kept), unname(km))),
        all_equal_roundtrip = isTRUE(all.equal(rt, unname(kept))),
        first_diff_live = first_diff, first_diff_roundtrip = first_rt_diff,
        live_n = length(km), rebuilt_n = length(kept),
        rebuilt_identical_to_live = identical(unname(kept), unname(km)),
        roundtrip_identical = identical(rt, unname(kept)),
        per_label = lapply(.s1$metas, length)
      )
    }
    .s1$knit_meta <- lapply(km, function(m) {
      list(class = class(m)[[1]], name = if (is.list(m)) m$name else NULL,
           version = if (is.list(m)) as.character(m$version) else NULL)
    })
  } else {
    knitr::knit("doc.Rmd", output = "out.md", quiet = TRUE, envir = globalenv())
  }
  "ok"
}, error = function(e) paste0("error: ", conditionMessage(e)))

md <- if (file.exists("out.md")) readLines("out.md", warn = FALSE) else character()
if (file.exists("out.md")) file.copy("out.md", file.path(outdir, sprintf("md.run%d.md", run)), overwrite = TRUE)
unlink("out.md")

jsonlite::write_json(list(
  fixture = basename(fixture), mechanism = mech, run = run,
  versions = as.list(.s1_versions), status = status,
  eng_r_deparsed_lines = .s1$eng_r_lines, knit_meta = .s1$knit_meta, meta_check = .s1$meta_check,
  target = .s1_format$pandoc$to, render_mode = .s1_render,
  events = .s1$events, inline_calls = .s1$inl, evaluate_inline_calls = .s1$ev,
  preserved = .s1$preserved, after_preserve_markers = .s1$after_preserve_markers
), file.path(outdir, sprintf("capture.run%d.json", run)),
auto_unbox = TRUE, null = "null", pretty = TRUE, force = TRUE)
