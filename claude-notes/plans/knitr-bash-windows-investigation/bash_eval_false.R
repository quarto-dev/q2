# Point bash at a program that cannot exist: if knitr launched it, the knit would fail.
knitr::knit_hooks$set(chunk = function(x, options) {
  message("chunk hook: engine=", options$engine, " eval=", options$eval)
  x
})
# error = FALSE: knit() keeps chunk errors in the output by default, which would hide a failed launch.
knitr::opts_chunk$set(engine.path = list(bash = "C:/does-not-exist/bash.exe"), error = FALSE)
rmd <- "```{bash b, eval=FALSE}\necho hi\n```\n"
invisible(knitr::knit(text = rmd, quiet = TRUE))
message("knit finished without launching bash")
