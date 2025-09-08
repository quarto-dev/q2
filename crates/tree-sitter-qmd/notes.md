
## TODO

- attribute handling in ATX headers

- equation handling

  - should it have attributes? Where?

    - Currently, we ask users to put the attribute after DisplayMath, like this:

      ```
      $$
      e = mc^2
      $$ {#eq-special-relativity}
      ```

      That syntax is inconsistent with other blocks. It could instead be

      ```
      $$ {#eq-special-relativity}
      e = mc^2
      $$
      ```

      The principle here would be something like "blocks get attributes after
      the opening bracket, and inlines get attributes after the closing bracket".

    - the real problem with equations is that users want to number individual
      equations inside a eqnarray* or something like that, and we have no mechanism to
      do it.

        - in addition, if we _do_ add support for in-block equation ids, we should consider that the output
          will not only need to exist for LaTeX, but will need to exist for html and typst as well.


## notes

- I really think Pandoc would benefit from the following rawblock extensions, and I 
  wonder if we should try to adopt this into Quarto somehow:

  - `{<format}` parses input from the rawblock as if it were Pandoc's `format`

    - This would be, then, how code opts into Quarto's html-table-is-ast feature.

  - 
    ```
    ::: {>format}
    :::
    ```

    process this output as if it were markdown, but only produce output in `format`.

    In Quarto, this is "just" syntax for

    ```
    ::: {.content-visible when-format="format"}
    :::
    ```
