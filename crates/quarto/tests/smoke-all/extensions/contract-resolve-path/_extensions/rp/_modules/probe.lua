-- Records what resolve_path returns at module load time. Per the
-- script-dir contract (GH #588), this must resolve against the extension
-- root -- the same answer the top-level script gets -- not against this
-- file's own directory.
return { resolved = quarto.utils.resolve_path("_modules/greet.lua") }
