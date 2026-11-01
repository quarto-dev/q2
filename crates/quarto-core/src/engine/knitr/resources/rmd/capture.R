# capture.R
# KNR-1 Phase 1B: record what knitr's evaluate() produced for each chunk, next
# to the unchanged markdown. Loaded by execute() only when the engine call asks
# for it (`capture`). Nothing here changes what knitr renders: the wrappers
# below call the original hooks and return their results untouched.
#
# Mechanism (plan S1): a wrapper around knit_hooks$evaluate records the pieces
# and selects plots with knitr's own rearrange_figs(); a wrapper around
# knit_hooks$chunk records the final options and joins everything into one
# per-chunk record in the S2 wire shape. The wrappers live in the knitr options
# list that execute() hands to rmarkdown, so rmarkdown restores the previous
# hooks when the render ends; the only state kept here is the `quarto_capture`
# environment.
#
# Record shape (plan S2/S3):
#   { ordinal, label, code, options, pieces: [{class, ...}], files }

quarto_capture <- new.env()
quarto_capture$records <- list()
quarto_capture$slot <- NULL
quarto_capture$ordinal <- 0L

# Options dropped because they carry document text or state outside the unit
# (S3); NULL-valued options are dropped too.
capture_option_skip <- c(
  "code", "params.src", "original.params.src", "yaml.code", "original.code",
  "hash", "fig.path", "cache.path", "fig.cur"
)

# Resolved chunk options as a JSON-able named list (S3 serialization rule):
# language values become their deparsed string, functions and environments are
# dropped and named in `unserializable`.
capture_options <- function(options) {
  out <- list()
  unserializable <- character()
  clean <- function(v) {
    if (is.function(v) || is.environment(v)) {
      return(NULL)
    }
    if (is.language(v)) {
      return(paste(deparse(v, width.cutoff = 500L), collapse = " "))
    }
    if (is.list(v)) {
      r <- lapply(v, clean)
      if (any(vapply(v, function(e) is.function(e) || is.environment(e), NA))) {
        return(NULL)
      }
      return(r)
    }
    if (is.atomic(v)) {
      return(if (is.factor(v)) as.character(v) else unname(v))
    }
    NULL
  }
  for (name in setdiff(names(options), capture_option_skip)) {
    v <- options[[name]]
    if (is.null(v)) {
      next
    }
    cv <- clean(v)
    if (is.null(cv)) {
      unserializable <- c(unserializable, name)
    } else {
      out[[name]] <- cv
    }
  }
  if (length(unserializable) > 0) {
    out[["unserializable"]] <- I(sort(unserializable))
  }
  if (length(out) == 0) {
    # an empty list would serialize as [] rather than {}
    out <- structure(list(), names = character())
  }
  out
}

# q2's wrap_asis_output (patch.R) fences text that looks like an HTML element
# and is not the kable-table div; the same test picks the mime type (S2).
capture_asis_mime <- function(text) {
  if (
    grepl("^<\\w+[ >]", text) &&
      grepl("<\\/\\w+>\\s*$", text) &&
      !grepl('^<div class="kable-table">', text)
  ) {
    "text/html"
  } else {
    "text/markdown"
  }
}

# The wrapper knitr's own msg_wrap uses: xfun::str_wrap from knitr 1.50 on, knitr's internal
# str_wrap before that (xfun 0.40 under knitr 1.44 has no str_wrap; found by the 1.44 corpus run).
capture_str_wrap <- function(text, width) {
  if (exists("str_wrap", envir = asNamespace("xfun"), inherits = FALSE)) {
    xfun::str_wrap(text, width = width)
  } else {
    get("str_wrap", envir = asNamespace("knitr"), inherits = FALSE)(text, width = width)
  }
}

capture_call_string <- function(condition) {
  cl <- conditionCall(condition)
  if (is.null(cl)) NULL else deparse(cl)[[1]]
}

# One evaluate() piece to its wire form. Anything unrecognized is class
# `other`, which makes the chunk a fallback unit downstream.
capture_piece <- function(p) {
  if (inherits(p, "source")) {
    return(list(class = "source", text = paste(p$src, collapse = "")))
  }
  # knit_asis and knit_image_paths inherit from character, and include_graphics()
  # output is both: test knit_image_paths first, then knit_asis
  if (inherits(p, "knit_image_paths")) {
    return(list(class = "image_paths", paths = I(as.character(p))))
  }
  if (inherits(p, "knit_asis")) {
    text <- paste(p, collapse = "")
    widget <-inherits(p, "knit_asis_htmlwidget")
    piece <- list(
      class = "asis",
      text = text,
      mime = if (widget) "text/html" else capture_asis_mime(text)
    )
    if (widget) {
      piece$widget <- TRUE
      meta <- attr(p, "knit_meta")
      if (length(meta) > 0) {
        piece$knit_meta <- as.character(jsonlite::serializeJSON(meta))
      }
    }
    return(piece)
  }
  if (is.character(p)) {
    return(list(class = "output", text = paste(p, collapse = "")))
  }
  if (inherits(p, "error")) {
    piece <- list(
      class = "error",
      text = as.character(p),
      ename = class(p)[[1]],
      evalue = conditionMessage(p)
    )
    piece$call <- capture_call_string(p)
    return(piece)
  }
  if (inherits(p, "warning")) {
    # as knitr formats it before comment_out (sew.warning + msg_wrap, S2)
    call <- capture_call_string(p)
    in_call <- if (is.null(call) || call == "eval(expr, envir, enclos)") {
      ""
    } else {
      paste(" in", call)
    }
    text <- sprintf("Warning%s: %s", in_call, conditionMessage(p))
    if (!length(grep("\n", text))) {
      text <- capture_str_wrap(text, width = getOption("width"))
    }
    piece <- list(class = "warning", text = text)
    piece$call <- call
    return(piece)
  }
  if (inherits(p, "message")) {
    return(list(class = "message", text = paste(conditionMessage(p), collapse = "")))
  }
  if (inherits(p, "recordedplot")) {
    return(list(class = "plot"))
  }
  list(class = "other", classes = I(class(p)))
}

# Plots that survive fig.keep, selected with knitr's own function at capture
# time (S1 finding 3). fig.show is left alone: it only moves plots.
capture_select_plots <- function(res, options) {
  keep <- options$fig.keep
  idx <- NULL
  if (is.logical(keep)) keep <- which(keep)
  if (is.numeric(keep)) {
    idx <- keep
    keep <- "index"
  }
  selected <- tryCatch(
    knitr:::rearrange_figs(res, keep, idx, "asis"),
    error = function(e) NULL
  )
  if (is.null(selected)) res else selected
}

capture_evaluate_hook <- function(orig) {
  function(code, ...) {
    plot_files_before <- length(knitr::opts_knit$get("plot_files"))
    res <- orig(code, ...)
    options <- knitr::opts_current$get()
    if (inherits(res, "list")) {
      selected <- capture_select_plots(res, options)
      quarto_capture$slot <- list(
        label = options$label,
        code = paste(code, collapse = "\n"),
        pieces = lapply(selected, capture_piece),
        plot_files_before = plot_files_before
      )
    }
    res
  }
}

capture_is_r_engine <- function(options) {
  identical(tolower(as.character(options$engine)), "r")
}

# Files knitr wrote for this chunk's plots (every device) plus the
# include_graphics paths the pieces carry; Rust reads their bytes.
capture_plot_files <- function(options, slot) {
  all <- knitr::opts_knit$get("plot_files")
  new <- if (length(all) > slot$plot_files_before) {
    all[(slot$plot_files_before + 1L):length(all)]
  } else {
    character()
  }
  # sew.recordedplot appends one entry per device for each plot (and knitr
  # repeats a path), so group by file stem in first-seen order and take each
  # group's first file: the first device's
  new <- unique(as.character(new))
  stems <- tools::file_path_sans_ext(new)
  per_plot <- split(new, factor(stems, levels = unique(stems)))
  list(all = new, first = vapply(per_plot, `[[`, "", 1L))
}

capture_chunk_hook <- function(prev) {
  function(x, options) {
    out <- prev(x, options)
    quarto_capture$ordinal <- quarto_capture$ordinal + 1L
    slot <- quarto_capture$slot
    quarto_capture$slot <- NULL
    if (!is.null(slot) && !identical(slot$label, options$label)) {
      slot <- NULL
    }
    files <- character()
    if (!capture_is_r_engine(options)) {
      pieces <- list(list(class = "other", classes = I(as.character(options$engine))))
      code <- paste(options$code, collapse = "\n")
    } else if (is.null(slot)) {
      # eval=FALSE and blank chunks never reach evaluate(): source only, from
      # the document text
      code <- paste(options$code, collapse = "\n")
      pieces <- if (nzchar(trimws(code))) {
        list(list(class = "source", text = paste0(code, "\n")))
      } else {
        list()
      }
    } else {
      code <- slot$code
      pf <- capture_plot_files(options, slot)
      files <- pf$all
      next_plot <- 0L
      pieces <- lapply(slot$pieces, function(p) {
        if (identical(p$class, "plot")) {
          next_plot <<- next_plot + 1L
          if (next_plot <= length(pf$first)) {
            p$files <- I(pf$first[[next_plot]])
          } else {
            # a plot with no file means knitr and the capture disagree
            p <- list(class = "other", classes = I("recordedplot"))
          }
        } else if (identical(p$class, "image_paths")) {
          files <<- c(files, p$paths)
        }
        p
      })
    }
    quarto_capture$records[[length(quarto_capture$records) + 1L]] <- list(
      ordinal = quarto_capture$ordinal,
      label = options$label,
      code = code,
      options = capture_options(options),
      pieces = unname(pieces),
      files = I(sort(unique(as.character(files))))
    )
    out
  }
}

# Return `knitr` (the options list execute() builds) with the capture wrappers
# installed. Also turns knitr's cache off for the recording run (S4): an
# unconditional copy of the hook execute.R installs for `cache: false`.
capture_install <- function(knitr) {
  quarto_capture$records <- list()
  quarto_capture$slot <- NULL
  quarto_capture$ordinal <- 0L
  hooks <- knitr$knit_hooks
  evaluate <- if (is.null(hooks[["evaluate"]])) {
    knitr::knit_hooks$get("evaluate")
  } else {
    hooks[["evaluate"]]
  }
  chunk <- if (is.null(hooks[["chunk"]])) {
    knitr::knit_hooks$get("chunk")
  } else {
    hooks[["chunk"]]
  }
  knitr$knit_hooks[["evaluate"]] <- capture_evaluate_hook(evaluate)
  knitr$knit_hooks[["chunk"]] <- capture_chunk_hook(chunk)
  knitr$opts_hooks$cache <- function(options) {
    options$cache <- FALSE
    options
  }
  knitr
}
