# Positron cache fixtures (RIU-3a)

Small, durable copies of Positron-format cache files and the documents they belong to, so the
import tests do not depend on a user's machine. `quarto_source` is a neutral `file://` URI and
`last_updated` is `1`; nothing else was modified.

| document | cache | provenance |
| --- | --- | --- |
| `python.qmd` | `python.cache.ipynb` | **Real.** Written by a Positron dev build (`code-oss-dev`) for `~/docs/inline-output/python.qmd`: two code cells, a stream and a result. The inline span in the document is not cached. The stream reads `sum: PUSHED-FROM-FILE` because that cache was written while a pushed notebook was being tried (the RIU-0 spikes); it is still a file Positron wrote. |
| `r.qmd` | `r.cache.ipynb` | **Real.** Written by Positron (Ark) for a three-cell R document: a labeled plot cell (stream, stream, image), an error cell, a result cell. The document was rebuilt from the `source` of each cached cell. R is not an acceptance case of the records-in-use epic (decision 11); this fixture exercises the knitr-owner gate only. |
| `julia.qmd` | `julia.cache.ipynb` | **Hand-built.** No Positron has produced a Julia cache. A stream, a string result and a multi-MIME plot bundle, shaped after the J5 and J7 findings (RIU-0b). The header says so (`q2_fixture_note`). Say "hand-built" wherever it is used. |

Copy, never modify, the originals in `~/Library/Application Support/`.
