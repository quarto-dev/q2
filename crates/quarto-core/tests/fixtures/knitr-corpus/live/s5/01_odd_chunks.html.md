



::: {.cell}

```{.r .cell-code}
child_value <- 41
child_value + 1
```

::: {.cell-output .cell-output-stdout}

```
[1] 42
```


:::
:::



::: {.cell}

```{.bash .cell-code}
echo "from the shell"
```


::: {.cell-output .cell-output-stdout}

```
from the shell
```


:::
:::





::: {.cell}

```{.sql .cell-code}
SELECT * FROM t
```


<div class="knitsql-table">


Table: 2 records

|a  |b  |
|:--|:--|
|1  |x  |
|2  |y  |

</div>
:::



::: {.cell}

```{.listeng .cell-code}
listeng source
```

::: {.cell-output .cell-output-stdout}

```
listeng stdout text
```


:::
:::



::: {.cell}

```{.r .cell-code}
"before override"
```

::: {.cell-output .cell-output-stdout}

```
[1] "before override"
```


:::
:::





::: {.cell}

```{.r .cell-code}
"after override"
```

OVERRIDDEN
[1] "after override"
:::

