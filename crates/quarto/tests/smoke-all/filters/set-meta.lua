-- `function Meta` doc-level handler E2E (bd-a9g50za2):
-- - replaces the YAML title,
-- - creates a new key from a bare Lua string,
-- - proves typewise order (element walk runs before Meta): the Meta
--   handler observes a nonzero Str count. The exact count is not pinned
--   because the walk traverses meta values too, and the *merged* config
--   metadata (format, filters list, ...) contributes many Strs.
local strs = 0
function Str(elem)
    strs = strs + 1
end
function Meta(meta)
    meta.title = pandoc.Inlines {
        pandoc.Str 'Title', pandoc.Space(),
        pandoc.Str 'From', pandoc.Space(), pandoc.Str 'Meta',
    }
    meta.subtitle = 'Subtitle from Meta handler (seen-' .. strs .. '-strs)'
    return meta
end
