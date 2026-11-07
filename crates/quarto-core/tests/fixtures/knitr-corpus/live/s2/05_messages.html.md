
::: {.cell}

```{.r .cell-code}
message("a message that is long enough that str_wrap at width 80 will fold it onto a second line of text")
```

::: {.cell-output .cell-output-stderr}

```
a message that is long enough that str_wrap at width 80 will fold it onto a second line of text
```


:::

```{.r .cell-code}
warning("a warning with a call")
```

::: {.cell-output .cell-output-stderr}

```
Warning: a warning with a call
```


:::

```{.r .cell-code}
f <- function() warning("warning inside f")
f()
```

::: {.cell-output .cell-output-stderr}

```
Warning in f(): warning inside f
```


:::

```{.r .cell-code}
stop("an error with a call")
```

::: {.cell-output .cell-output-error}

```
Error: an error with a call
```


:::
:::



::: {.cell}

```{.r .cell-code}
options(width = 40)
message("a message that is long enough that str_wrap at width forty will fold it a few times")
```

::: {.cell-output .cell-output-stderr}

```
a message that is long enough that str_wrap at width forty will fold it a few times
```


:::
:::



::: {.cell}

```{.r .cell-code}
options(width = 80)
warning("a warning message that is long enough that str_wrap at width eighty will fold it onto a second line")
```

::: {.cell-output .cell-output-stderr}

```
Warning: a warning message that is long enough that str_wrap at width eighty
will fold it onto a second line
```


:::
:::



::: {.cell}

```{.r .cell-code}
options(width = 40)
warning("a warning message that is long enough that str_wrap at width forty will fold it a few times")
```

::: {.cell-output .cell-output-stderr}

```
Warning: a warning message that is long
enough that str_wrap at width forty
will fold it a few times
```


:::
:::

