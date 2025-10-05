## TODO

- Allow attributes in editor marks (importantly for date/author)

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

## parsing differences

- Lines that end with spaces might be interpreted differently.


## Things to handle from quarto-web we're leaving behind for now

### docs/advanced/typst/typst-css.qmd

|                  | span | div | table | td  |
|------------------|------|-----|-------|-----|
| background-color |   ✓  |  ✓  |       |  ✓  |
| border[^1]       |      |     |       |  ✓  |
| color            |   ✓  |  ✓  |       |  ✓  |
| font-family      |      |  ✓  |   ✓   |     |
| font-size        |      |  ✓  |   ✓   |     |
| opacity          |   ✓  |     |       |  ✓  |
| align[^2]        |      |     |       |  ✓  |


[^1]: `border`, `border-left` etc, `border-width`, `border-style`, `border-color`, `border-left-width` etc

[^2]: `text-align`, `vertical-align`
