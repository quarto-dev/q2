# q2 leaves shortcodes unevaluated inside fenced code blocks

**Observed with:** q2 0.15.0
**Repro:** `q2 render repro.qmd` in this directory.

## Expected (Quarto 1)

Quarto 1 substitutes shortcodes inside fenced code blocks (no special
attribute needed). The `.ini` example renders as:

```
[Section "corporate LDAP"]
```

Verified with Quarto 1 (99.9.9 dev).

## Actual (q2 0.15.0)

Body-text shortcodes substitute fine, but inside the fence the
shortcode stays literal (HTML-escaped in output):

```
[Section "corporate {{< meta vendor >}}"]
```

No warning is emitted.

## Impact on the Connect docs

`admin/authentication/ldap-based/include/_users.qmd` (shared by the
five LDAP authentication pages) uses `{{< meta authentication.vendor >}}`
inside a `.gcfg` config example — the rendered configuration snippet
shows the raw shortcode instead of the vendor name.
