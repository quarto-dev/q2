-- `function Meta` doc-level handler E2E (bd-a9g50za2):
-- - replaces the YAML title,
-- - creates a new key from a bare Lua string,
-- - proves typewise order (element walk runs before Meta).
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
