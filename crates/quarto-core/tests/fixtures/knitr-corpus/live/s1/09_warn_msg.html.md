
::: {.cell}

```{.r .cell-code}
message("m1")
```

::: {.cell-output .cell-output-stderr}

```
m1
```


:::

```{.r .cell-code}
warning("w1")
```

::: {.cell-output .cell-output-stderr}

```
Warning: w1
```


:::

```{.r .cell-code}
message("m2")
```

::: {.cell-output .cell-output-stderr}

```
m2
```


:::

```{.r .cell-code}
warning("w2")
```

::: {.cell-output .cell-output-stderr}

```
Warning: w2
```


:::
:::



::: {.cell}

```{.r .cell-code}
warning("dropped warning")
print("x")
```

::: {.cell-output .cell-output-stdout}

```
[1] "x"
```


:::
:::



::: {.cell}

```{.r .cell-code}
warning("na warning")
print("y")
```

::: {.cell-output .cell-output-stdout}

```
[1] "y"
```


:::
:::



::: {.cell}

```{.r .cell-code}
message("dropped message")
print("z")
```

::: {.cell-output .cell-output-stdout}

```
[1] "z"
```


:::
:::



::: {.cell}

```{.r .cell-code}
message("na message")
print("w")
```

::: {.cell-output .cell-output-stdout}

```
[1] "w"
```


:::
:::



::: {.cell}

```{.r .cell-code}
warning("first")
```

::: {.cell-output .cell-output-stderr}

```
Warning: first
```


:::

```{.r .cell-code}
warning("second")
```
:::



::: {.cell}

```{.r .cell-code}
message("one\n")
message("two\n")
```

::: {.cell-output .cell-output-stderr}

```
two
```


:::

```{.r .cell-code}
message("three\n")
```
:::

