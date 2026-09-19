-- quarto2-shim.lua
-- QUARTO2-PATCH (q2 pandoc-hybrid, P4): placeholder for P5's wire-format
-- shim. Spliced into quarto_filter_list between quarto_init_filters and
-- quarto_normalize_filters -- see resources/pandoc-filters/README.md.
-- P5 replaces this file's body with the real conversion; P4 ships only the
-- group shape (an empty filter, so main.lua loads and runs unchanged).

quarto_pandoc_shim_filters = {
  { name = "quarto2-wire-shim",
    filter = {},          -- P5 fills this in; run_emulated_filter short-circuits
                          -- on an empty filter (ast/customnodes.lua:92-94)
    traverser = 'jog',    -- the walker, matching every main.lua entry
    -- No top-down traversal flag on the filter table: the contract is
    -- BOTTOM-UP, so a nested wire node (a FloatRefTarget inside a Callout's
    -- content slot) is converted inner-first and the outer constructor
    -- receives a real Q1 scaffold. The traversal-direction flag is a
    -- property of the *filter* table (ast/runemulation.lua:132 is the only
    -- top-down user in the tree); `traverser` above selects the *walker*
    -- (ast/customnodes.lua:76-88) and is a separate concern.
  }
}
