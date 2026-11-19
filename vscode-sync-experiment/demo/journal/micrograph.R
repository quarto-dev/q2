# Generates figures/micrograph-main.png and micrograph-feature.png (the same network, recolored on `feature`).
# Run once: Rscript micrograph.R   (the PNGs are committed; the journal builder does not run R.)
make <- function(file, hypha, node) {
  set.seed(1717)
  png(file, width = 640, height = 400, res = 96, bg = "#0d1a12")
  par(mar = c(0, 0, 0, 0))
  plot.new(); plot.window(xlim = c(0, 1), ylim = c(0, 1))
  n <- 46
  x <- runif(n); y <- runif(n)
  d <- as.matrix(dist(cbind(x, y)))
  for (i in seq_len(n)) for (j in seq_len(n)) {
    if (i < j && d[i, j] < 0.2) segments(x[i], y[i], x[j], y[j], col = hypha, lwd = 1 + 2 * (0.2 - d[i, j]) / 0.2)
  }
  points(x, y, pch = 21, bg = node, col = hypha, cex = 1.1)
  text(0.02, 0.03, "10 µm", col = "#e8f2ea", adj = c(0, 0), cex = 0.9)
  dev.off()
}
make("micrograph-main.png", "#7fd6a0", "#e8f2ea")
make("micrograph-feature.png", "#f2a65a", "#fff3e0")
