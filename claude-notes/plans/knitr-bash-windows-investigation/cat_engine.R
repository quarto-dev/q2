rmd <- "```{r a}\n1\n```\n\n```{cat b}\necho hi\n```\n"
out <- knitr::knit(text = rmd, quiet = TRUE)
cat(out, sep = "\n")
