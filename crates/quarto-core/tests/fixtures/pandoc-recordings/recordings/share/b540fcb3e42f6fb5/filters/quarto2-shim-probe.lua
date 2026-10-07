-- quarto2-shim-probe.lua
-- QUARTO2-PATCH (q2 pandoc-hybrid, P5): an observer `-L` filter, appended
-- *after* `main.lua` by the test harness's `run_main_lua_capturing_ast`
-- only (crates/quarto-core/src/pandoc_filters/harness.rs) -- never part of
-- the production filter chain. Writes the post-`main.lua`-filter AST to
-- QUARTO2_SHIM_PROBE_OUT as Pandoc JSON and returns `doc` unchanged, so the
-- real writer (docx, etc.) still produces its normal output from the same
-- pandoc invocation.
return {
  {
    Pandoc = function(doc)
      local out = os.getenv("QUARTO2_SHIM_PROBE_OUT")
      if out ~= nil then
        local f = io.open(out, "w")
        if f then
          f:write(pandoc.write(doc, "json"))
          f:close()
        end
      end
      return doc
    end
  }
}
