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
quarto_capture$hooks_at_start <- NULL
quarto_capture$hook_override <- NULL
quarto_capture$cache_asked <- character()
quarto_capture$active <- FALSE
quarto_capture$pending <- NULL
quarto_capture$orig_cache <- NULL

# The knit hooks whose replacement silently changes the markdown the presenter would not see
# (KNR-1 S5). `chunk`, `document` and `inline` are Quarto's own.
capture_watched_hooks <- c("source", "output", "message", "warning", "error", "plot")

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

# knitr >= 1.51 passes rlang::entrace() to evaluate as an error handler when
# rlang is installed (yihui/knitr#2388). entrace() re-signals every error as a
# new `rlang_error`, keeping the original condition in `$error` and dropping
# or replacing its call. Unwrap it so the record does not depend on the knitr
# version. An error raised with rlang::abort() has no `$error` and is kept.
capture_raised_error <- function(p) {
  if (inherits(p, "rlang_error") && inherits(p$error, "error")) p$error else p
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
    piece <- list(class = "image_paths", paths = I(as.character(p)))
    if (!is.null(attr(p, "dpi"))) piece$dpi <- attr(p, "dpi")
    return(piece)
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
    # name the condition the author raised; text stays as knitr formats it
    raised <- capture_raised_error(p)
    piece <- list(
      class = "error",
      text = as.character(p),
      ename = class(raised)[[1]],
      evalue = conditionMessage(raised)
    )
    piece$call <- capture_call_string(raised)
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
    if (is.null(quarto_capture$hooks_at_start)) {
      quarto_capture$hooks_at_start <- lapply(
        stats::setNames(capture_watched_hooks, capture_watched_hooks),
        function(n) knitr::knit_hooks$get(n)
      )
    }
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

# The width `sew.knit_image_paths` gives each include_graphics() path (its
# raster_dpi_width) when the chunk sets no out.width; NA where knitr has none.
capture_image_widths <- function(paths, dpi, options) {
  retina <- options$fig.retina
  out_width <- options[["out.width"]]
  if (is.numeric(retina)) {
    w2 <- options$fig.width * options$dpi / retina
    if (length(out_width) * length(w2) == 1 && is.numeric(out_width) && out_width == w2) {
      out_width <- NULL
    }
  }
  if (is.null(dpi)) dpi <- options$dpi
  vapply(paths, function(path) {
    if (!is.null(out_width)) return(NA_real_)
    w <- tryCatch(
      get("raster_dpi_width", envir = asNamespace("knitr"), inherits = FALSE)(path, dpi),
      error = function(e) NULL
    )
    if (is.numeric(w) && length(w) == 1L) as.numeric(w) else NA_real_
  }, numeric(1), USE.NAMES = FALSE)
}

# Compare each watched knit hook with the one in place before the first chunk ran; the first
# difference sticks, so that chunk and every later one is flagged (hooks are global and
# persist). An override reverted later stays flagged.
capture_check_hooks <- function() {
  if (!is.null(quarto_capture$hook_override) || is.null(quarto_capture$hooks_at_start)) {
    return(invisible())
  }
  for (n in capture_watched_hooks) {
    if (!identical(knitr::knit_hooks$get(n), quarto_capture$hooks_at_start[[n]])) {
      quarto_capture$hook_override <- n
      break
    }
  }
  invisible()
}

# A `child=` chunk has no evaluate() pieces and no chunk-hook call; hooks.R reports it here.
# It becomes a captured record whose one piece is `other` (class "child"), which the adapter
# turns into a fallback cell holding the child's finished markdown.
capture_child_record <- function(options, markdown) {
  capture_check_hooks()
  quarto_capture$ordinal <- quarto_capture$ordinal + 1L
  record <- list(
    ordinal = quarto_capture$ordinal,
    label = options$label,
    code = "",
    options = capture_options(options),
    pieces = list(list(class = "other", classes = I("child"))),
    files = I(character())
  )
  if (!is.null(quarto_capture$hook_override)) {
    record$hook_override <- quarto_capture$hook_override
  }
  quarto_capture$records[[length(quarto_capture$records) + 1L]] <- record
  invisible()
}

# The marker knitr's skeleton holds where a unit belongs: `<!-- q2:<nonce>:<unit id> -->`
# (engine/knitr/skeleton.rs reads it). Only a marker with this run's nonce counts.
capture_marker <- function(id) {
  paste0("<!-- q2:", quarto_capture$nonce, ":", id, " -->")
}

# A chunk asking for `cache = TRUE` (knitr's level 3, the one that stores the chunk hook's
# output). Levels 1 and 2 store the evaluate() list and are not recorded (OPR-7e).
# knitr normalizes TRUE to 3 only after the cache lookup, so a hit still sees TRUE.
capture_is_level3 <- function(options) {
  isTRUE(options$cache) || isTRUE(as.numeric(options$cache) == 3)
}

# The chunk record hooks.R made for `id` (the last one: a chunk id is unique in a run).
capture_find_chunk_record <- function(id) {
  recs <- quarto_chunk_records$records
  for (i in rev(seq_along(recs))) {
    if (identical(recs[[i]]$id, id)) {
      return(recs[[i]])
    }
  }
  NULL
}

# Units that asked for the cache and ended the run with no record: level-1 and level-2 chunks,
# an `include: false` level-3 hit, and a level-3 hit whose sidecar is missing or fails a check.
# Rust leaves them out of the expected unit count.
capture_cached_units <- function() {
  recorded <- vapply(
    quarto_capture$records,
    function(r) as.character(r$options[["quarto-unit-id"]] %||% ""),
    ""
  )
  setdiff(quarto_capture$cache_asked, recorded)
}

# --- level-3 sidecar (OPR-7e) -----------------------------------------------------------
# knitr keeps a cache entry's chunk output as a string and returns it on a hit without calling
# any hook, so the capture record is saved in a file beside the entry (`<hash>.q2rec.json`) and
# read back on a hit. Only `cache$save` and `cache$output` are wrapped, in the knitr namespace.
# A sidecar is a hint: a missing file, another version, another id, a changed `include` or a
# digest that does not match the stored markdown leaves the chunk unrecorded, with knitr's own
# markdown in the document (OPR-7b's behavior). knitr's own invalidation (cache.rebuild,
# dependson) removes only the entry's data files, so a stale sidecar is possible; the digest
# check makes it harmless.

capture_sidecar_version <- 1L

capture_sidecar_path <- function(hash) paste0(hash, ".q2rec.json")

# jsonlite writes an NA option (`comment = NA`) as null and reads null back as NULL, which it
# would write as {}; the records never hold a NULL (capture_options drops them), so every null
# in a sidecar was an NA.
capture_nulls_to_na <- function(x) {
  if (is.null(x)) {
    return(NA)
  }
  if (is.list(x)) {
    return(lapply(x, capture_nulls_to_na))
  }
  x
}

capture_digest <- function(text) {
  f <- tempfile()
  on.exit(unlink(f))
  writeBin(charToRaw(enc2utf8(text)), f)
  unname(tools::md5sum(f))
}

capture_cache_save <- function(keys, outname, hash, lazy = TRUE) {
  orig <- quarto_capture$orig_cache
  pending <- quarto_capture$pending
  options <- knitr::opts_current$get()
  quarto_capture$pending <- NULL
  if (
    isTRUE(quarto_capture$active) &&
      !is.null(pending) &&
      identical(pending$id, options[["quarto-unit-id"]]) &&
      !is.null(pending$chunk)
  ) {
    # knitr stores what the chunk hook returned, which with a nonce is this run's marker line;
    # store the finished markdown instead, so a later run can put its own marker in its place
    assign(outname, pending$markdown, envir = knitr::knit_global())
    side <- list(
      version = capture_sidecar_version,
      id = pending$id,
      include = pending$include,
      digest = capture_digest(pending$markdown),
      record = pending$record,
      chunk = pending$chunk
    )
    tryCatch(
      writeLines(
        jsonlite::toJSON(side, auto_unbox = TRUE, null = "null", digits = NA, force = TRUE),
        capture_sidecar_path(hash)
      ),
      error = function(e) NULL
    )
  }
  orig$save(keys, outname, hash, lazy = lazy)
}

# The stored markdown of a level-3 hit, with the chunk recorded from its sidecar. Returns the
# string knitr should put in the document, or NULL to leave the chunk unrecorded.
capture_adopt_hit <- function(hash, stored) {
  options <- knitr::opts_current$get()
  id <- options[["quarto-unit-id"]]
  path <- capture_sidecar_path(hash)
  if (is.null(id) || !capture_is_level3(options) || !file.exists(path)) {
    return(NULL)
  }
  side <- capture_nulls_to_na(jsonlite::fromJSON(path, simplifyVector = FALSE))
  markdown <- paste(stored, collapse = "")
  capture_check_hooks()
  ok <- identical(side$version, capture_sidecar_version) &&
    identical(side$id, id) &&
    identical(side$chunk$id, id) &&
    identical(side$record$options[["quarto-unit-id"]], id) &&
    identical(side$include, isTRUE(options$include)) &&
    identical(side$digest, capture_digest(markdown)) &&
    is.null(side$record$hook_override) &&
    is.null(quarto_capture$hook_override)
  if (!isTRUE(ok)) {
    return(NULL)
  }
  quarto_capture$ordinal <- quarto_capture$ordinal + 1L
  record <- side$record
  record$ordinal <- quarto_capture$ordinal
  quarto_capture$records[[length(quarto_capture$records) + 1L]] <- record
  quarto_chunk_records$records[[length(quarto_chunk_records$records) + 1L]] <- side$chunk
  if (!is.null(quarto_capture$nonce)) {
    return(paste0(options[["indent"]], capture_marker(id), "\n"))
  }
  stored
}

capture_cache_output <- function(hash, mode = "character") {
  orig <- quarto_capture$orig_cache
  out <- orig$output(hash, mode)
  if (!isTRUE(quarto_capture$active) || !identical(mode, "character")) {
    return(out)
  }
  adopted <- tryCatch(capture_adopt_hit(hash, out), error = function(e) NULL)
  if (is.null(adopted)) out else adopted
}

# Wrap `cache$save` and `cache$output` in knitr's namespace. Idempotent (the pristine list is
# kept once), and the wrappers do nothing unless `quarto_capture$active`. The engine runs one
# Rscript per call, so the patch dies with the process.
capture_patch_cache <- function() {
  ns <- asNamespace("knitr")
  if (is.null(quarto_capture$orig_cache)) {
    quarto_capture$orig_cache <- get("cache", envir = ns)
  }
  patched <- quarto_capture$orig_cache
  patched$save <- capture_cache_save
  patched$output <- capture_cache_output
  assignInNamespace("cache", patched, ns)
}

capture_chunk_hook <- function(prev) {
  function(x, options) {
    out <- prev(x, options)
    id <- options[["quarto-unit-id"]]
    level3 <- capture_is_level3(options)
    if (!is.null(id) && !level3 && id %in% quarto_capture$cache_asked) {
      # a level-1 or level-2 cached chunk (OPR-7b): not recorded, and knitr's own markdown stays
      # in the skeleton. On a hit this hook runs with no `evaluate` behind it. (A level-3 hit
      # never reaches this hook: `capture_cache_output` records it from its sidecar, OPR-7e.)
      quarto_capture$slot <- NULL
      return(out)
    }
    capture_check_hooks()
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
          p$widths <- I(capture_image_widths(p$paths, p$dpi, options))
          p$dpi <- NULL
        }
        p
      })
    }
    record <- list(
      ordinal = quarto_capture$ordinal,
      label = options$label,
      code = code,
      options = capture_options(options),
      pieces = unname(pieces),
      files = I(sort(unique(as.character(files))))
    )
    # a NULL would serialize as `{}`: add the field only when set
    if (!is.null(quarto_capture$hook_override)) {
      record$hook_override <- quarto_capture$hook_override
    }
    quarto_capture$records[[length(quarto_capture$records) + 1L]] <- record
    quarto_capture$pending <- NULL
    if (level3 && !is.null(id) && capture_is_r_engine(options)) {
      # the sidecar `capture_cache_save` writes when knitr stores this chunk's output (OPR-7e)
      quarto_capture$pending <- list(
        id = id,
        record = record,
        chunk = capture_find_chunk_record(id),
        include = isTRUE(options$include),
        markdown = paste(out, collapse = "")
      )
    }
    # KNR-6: with a nonce, a chunk Rust may present returns a marker line instead of its
    # markdown (the markdown still travels in `chunks`, keyed by unit id, hooks.R). The same
    # trailing newline as the hook's own output keeps the skeleton's line structure, so Rust
    # can swap the finished markdown back in for a chunk it cannot present. A chunk whose hook
    # is overridden, that is not R, or that carries no unit id keeps its real markdown.
    if (
      !is.null(quarto_capture$nonce) &&
        !is.null(id) &&
        capture_is_r_engine(options) &&
        is.null(quarto_capture$hook_override)
    ) {
      return(paste0(options[["indent"]], capture_marker(id), "\n"))
    }
    out
  }
}

# The option names the chunk hook does NOT forward as cell attributes (KNR-3): the same
# `known_opts` union hooks.R builds in its chunk hook ("forward any other unknown attributes"),
# so the presenter forwards exactly what Quarto 1 does. Read before any chunk runs, so a setup
# chunk's `opts_chunk$set` cannot make an unknown name known. `format_opts_chunk` is the
# `opts_chunk` list of the options execute() builds (ft.shadow, dev.args, crop, the document's
# `knitr: opts_chunk`): knitr applies it before the first chunk, so the live hook sees those
# names as known. Keep in step with hooks.R.
capture_known_options <- function(format_opts_chunk = list()) {
  knitr_default_opts <- unique(c(
    tryCatch(names(knitr:::opts_chunk_attr), error = function(e) character()),
    names(knitr::opts_chunk$get()),
    names(format_opts_chunk)
  ))
  # fmt: skip
  quarto_knitr_opts <- c(
    "fig.cap", "fig.subcap", "fig.scap", "fig.link", "fig.alt",
    "fig.align", "fig.env", "fig.pos", "fig.num", "out.width"
  )
  # fmt: skip
  quarto_opts <- c(
    "label", "lst-cap", "lst-label", "classes", "panel", "column",
    "tbl-column", "tbl-cap-location", "cap-location", "code-fold",
    "code-summary", "code-overflow", "code-line-numbers",
    "layout", "layout-nrow", "layout-ncol", "layout-align", "layout-valign",
    "output", "html-table-processing",
    "fig-column", "fig.column", "fig-cap-location", "fig.cap-location",
    "fig-format", "fig.format", "fig-dpi", "fig.dpi",
    "include.hidden", "source.hidden", "plot.hidden",
    "output.hidden", "warning.hidden", "message.hidden"
  )
  # fmt: skip
  other_opts <- c(
    "eval", "yaml.code", "code", "file", "params.src", "original.params.src",
    "original.code",
    "fenced.echo", "chunk.echo", "lang", "out.width.px", "out.height.px",
    "indent", "class.source", "class.output", "class.message",
    "class.warning", "class.error", "attr.source", "attr.output",
    "attr.message", "attr.warning", "attr.error", "connection", "hash",
    "quarto-unit-id"
  )
  unique(c(knitr_default_opts, quarto_knitr_opts, quarto_opts, other_opts))
}

# Return `knitr` (the options list execute() builds) with the capture wrappers
# installed. A chunk that asks for knitr's cache keeps it (OPR-7b, reversing S4): its unit id is
# noted in `quarto_capture$cache_asked` and the chunk hook leaves it unrecorded, with knitr's real
# markdown in place; Rust excludes those ids from the expected unit count.
capture_install <- function(knitr, nonce = NULL) {
  quarto_capture$nonce <- nonce
  quarto_capture$records <- list()
  quarto_capture$slot <- NULL
  quarto_capture$ordinal <- 0L
  quarto_capture$hooks_at_start <- NULL
  quarto_capture$hook_override <- NULL
  quarto_capture$cache_asked <- character()
  quarto_capture$pending <- NULL
  quarto_capture$active <- TRUE
  capture_patch_cache()
  quarto_capture$known_options <- capture_known_options(knitr$opts_chunk)
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
  quarto_child_state$on_child <- capture_child_record
  knitr$knit_hooks[["evaluate"]] <- capture_evaluate_hook(evaluate)
  knitr$knit_hooks[["chunk"]] <- capture_chunk_hook(chunk)
  # `opts_hooks$cache` is the one hook knitr runs for every chunk on a miss and on a hit, at every
  # cache level (a hit skips `evaluate`, and level 3 skips the chunk hook too). Keep execute.R's own
  # hook (`cache: false` forces the cache off), then note each chunk still asking for the cache.
  prev_cache_hook <- knitr$opts_hooks$cache
  knitr$opts_hooks$cache <- function(options) {
    if (is.function(prev_cache_hook)) {
      options <- prev_cache_hook(options)
    }
    id <- options[["quarto-unit-id"]]
    if (
      !is.null(id) &&
        !is.null(options$cache) &&
        !isFALSE(options$cache) &&
        !identical(options$cache, 0)
    ) {
      quarto_capture$cache_asked <- unique(c(quarto_capture$cache_asked, as.character(id)))
    }
    options
  }
  knitr
}
