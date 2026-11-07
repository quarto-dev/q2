
::: {.cell}

```{.r .cell-code}
knitr::kable(head(data.frame(a = 1:2, b = c("x", "y"))))
```

::: {.cell-output-display}


|  a|b  |
|--:|:--|
|  1|x  |
|  2|y  |


:::
:::



::: {.cell}

```{.r .cell-code}
knitr::asis_output("**raw markdown from asis_output**")
```

::: {.cell-output-display}
**raw markdown from asis_output**
:::
:::



::: {.cell}

```{.r .cell-code}
knitr::asis_output("<div class='raw'>raw html</div>")
```

::: {.cell-output-display}
`````{=html}
<div class='raw'>raw html</div>
`````
:::
:::



::: {.cell}

```{.r .cell-code}
knitr::asis_output("dropped by results=hide")
```
:::

