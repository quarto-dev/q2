
::: {.cell}

```{.r .cell-code}
png("made.png", width = 200, height = 200)
plot(1:3)
invisible(dev.off())
```
:::



::: {.cell}

```{.r .cell-code}
knitr::include_graphics("made.png")
```

::: {.cell-output-display}
![](made.png){width=200}
:::
:::

