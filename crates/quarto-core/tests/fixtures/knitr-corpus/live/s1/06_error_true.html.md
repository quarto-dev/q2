
::: {.cell}

```{.r .cell-code}
a <- 1
stop("boom")
```

::: {.cell-output .cell-output-error}

```
Error: boom
```


:::

```{.r .cell-code}
b <- 2
print("after error")
```

::: {.cell-output .cell-output-stdout}

```
[1] "after error"
```


:::
:::



::: {.cell}

```{.r .cell-code}
exists("b")
```

::: {.cell-output .cell-output-stdout}

```
[1] TRUE
```


:::
:::

