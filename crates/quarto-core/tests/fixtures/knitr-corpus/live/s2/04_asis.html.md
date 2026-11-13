
```{.r .cell-code}
cat("**markdown from cat**\n\n")
```

**markdown from cat**

```{.r .cell-code}
cat("- item a\n- item b\n")
```

- item a
- item b


::: {.cell}

```{.r .cell-code}
knitr::asis_output("**markdown from asis_output**")
```

::: {.cell-output-display}
**markdown from asis_output**
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



```{.r .cell-code}
knitr::asis_output("<div class='raw'>raw html under results asis</div>")
```

`````{=html}
<div class='raw'>raw html under results asis</div>
`````
