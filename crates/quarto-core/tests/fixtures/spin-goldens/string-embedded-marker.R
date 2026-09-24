#' A multi-line string containing text that looks like markers.

x <- "line one
#' not a doc line
#+ not a chunk marker
line four"
print(x)
