; Synthetic equal-extent collision fixture.
;
; The two patterns below BOTH match the same `bare_key` node, so a
; `Query.captures()` walk emits two captures at the IDENTICAL byte range
; (same start AND same end). That is a genuine *equal-extent* collision —
; the one residual ambiguity `flatten_spans` must tie-break, because
; "innermost (narrowest) wins" cannot order two spans of equal length.
;
; This is precisely the case the upstream tree-sitter-toml query does NOT
; exercise: there `(bare_key) @type` is *nested inside*
; `(pair (bare_key)) @property` (different ends — type ⊂ property), so node-exact extraction resolves it
; by innermost-wins with no tie. Those captures only LOOK equal-extent in
; the legacy `collect_spans` output, where the bd-98k6 over-wrap bug
; stretched `type` to the pair's end.

(bare_key) @type
(bare_key) @property

"=" @operator

(integer) @number
