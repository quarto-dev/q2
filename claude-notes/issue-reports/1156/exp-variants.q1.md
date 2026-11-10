---
title: Asis variants
---

## A: stdout plus an execute_result

Printed **markdown**.

```
'a trailing value'
```


## B: stderr and an IPython Markdown display

```
to stderr
```
Displayed *markdown*.


## C: echo true

``` {.python .cell-code}
print("Echoed cell, **bold** output.")
```
Echoed cell, **bold** output.


## D: a class forces the cell div (needCell)

::: {#103a2bd2 .cell .keepme execution_count=4}
Inside a **div**.
:::


## E: a tabset (the reporter's use case)

::: {.panel-tabset}

## pip

```bash
pip install thing
```

## conda

```bash
conda install thing
```

:::


