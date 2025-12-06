-- Lua filter to convert Pandoc DefinitionList AST nodes to div-based definition lists
-- This produces output in the definition-list div syntax used by quarto-markdown

if PANDOC_VERSION and PANDOC_VERSION.must_be_at_least then
    PANDOC_VERSION:must_be_at_least("2.11")
else
    error("pandoc version >=2.11 is required")
end

-- Convert a DefinitionList to a div with .definition-list class
local function definition_list_to_div(def_list)
    -- Build div attributes with .definition-list class
    local div_attr = pandoc.Attr('', {'definition-list'}, {})

    -- Build the outer bullet list containing all term-definition pairs
    local outer_items = {}

    -- Each item in the definition list is a tuple: (term, definitions)
    -- term: list of inline elements
    -- definitions: list of definition blocks (each definition is a list of blocks)
    for _, item in ipairs(def_list.content) do
        local term = item[1]  -- List of inline elements
        local definitions = item[2]  -- List of definition blocks

        -- Create the inner bullet list containing the definitions
        local def_items = {}
        for _, def_blocks in ipairs(definitions) do
            -- Each definition is a list of blocks
            -- Clone the blocks to avoid modifying the original
            local blocks = pandoc.Blocks({})
            for _, block in ipairs(def_blocks) do
                table.insert(blocks, block:clone())
            end

            -- Ensure we have at least one block
            if #blocks == 0 then
                blocks = pandoc.Blocks({pandoc.Para({})})
            end

            table.insert(def_items, blocks)
        end

        -- Create a bullet list for the definitions
        local def_list_elem = pandoc.BulletList(def_items)

        -- Create the outer list item containing:
        -- 1. The term as a paragraph
        -- 2. The nested bullet list of definitions
        local term_para = pandoc.Para(term)
        table.insert(outer_items, {term_para, def_list_elem})
    end

    -- Create the outer bullet list (list of term-definition pairs)
    local outer_list = pandoc.BulletList(outer_items)

    -- Create the div containing the outer list
    return pandoc.Div({outer_list}, div_attr)
end

return {{DefinitionList = definition_list_to_div}}
