-- Lua filter to convert Pandoc grid tables to list-table format
-- This produces output that can be processed by list-table.lua

if PANDOC_VERSION and PANDOC_VERSION.must_be_at_least then
    PANDOC_VERSION:must_be_at_least("2.11")
else
    error("pandoc version >=2.11 is required")
end

-- Convert alignment enum to character code
local function alignment_to_char(align)
    local align_str = tostring(align)
    if align_str == 'AlignLeft' then return 'l'
    elseif align_str == 'AlignRight' then return 'r'
    elseif align_str == 'AlignCenter' then return 'c'
    else return 'd' end
end

-- Convert a cell to a list of blocks with optional attribute span prepended
local function cell_to_blocks(cell)
    -- Extract cell properties using Lua API
    local contents = cell.contents
    local align = cell.alignment
    local rowspan = cell.row_span
    local colspan = cell.col_span
    local attr = cell.attr

    -- Clone the blocks to avoid modifying the original
    local blocks = pandoc.Blocks({})
    for _, block in ipairs(contents) do
        table.insert(blocks, block:clone())
    end

    -- If we have non-default cell attributes, prepend an empty span
    local align_str = tostring(align)
    if rowspan ~= 1 or colspan ~= 1 or align_str ~= 'AlignDefault' then
        local span_attr = pandoc.Attr('', {}, {})
        if colspan ~= 1 then
            span_attr.attributes.colspan = tostring(colspan)
        end
        if rowspan ~= 1 then
            span_attr.attributes.rowspan = tostring(rowspan)
        end
        if align_str ~= 'AlignDefault' then
            span_attr.attributes.align = alignment_to_char(align)
        end

        local empty_span = pandoc.Span({}, span_attr)

        -- Insert the empty span at the beginning of the first block's content
        if #blocks > 0 and blocks[1].content then
            table.insert(blocks[1].content, 1, empty_span)
        else
            -- If there's no content, create a paragraph with just the span
            blocks = pandoc.Blocks({pandoc.Para({empty_span})})
        end
    end

    -- Ensure we have at least one block
    if #blocks == 0 then
        blocks = pandoc.Blocks({pandoc.Para({})})
    end

    return blocks
end

-- Convert a Pandoc Table to a list-table Div
local function table_to_list_table(tbl)
    -- Extract table components using Lua API
    local attr = tbl.attr
    local caption = tbl.caption
    local colspecs = tbl.colspecs
    local thead = tbl.head
    local tbodies = tbl.bodies
    local tfoot = tbl.foot

    -- Build div attributes, starting from table attributes
    local div_attr = pandoc.Attr(attr.identifier, {'list-table'}, {})

    -- Copy table classes
    for _, class in ipairs(attr.classes) do
        table.insert(div_attr.classes, class)
    end

    -- Copy table attributes
    for k, v in pairs(attr.attributes) do
        div_attr.attributes[k] = v
    end

    -- Count header rows from thead
    local thead_rows = thead.rows
    local header_row_count = #thead_rows
    if header_row_count > 0 then
        div_attr.attributes['header-rows'] = tostring(header_row_count)
    end

    -- Extract alignments and widths from colspecs
    local aligns = {}
    local widths = {}
    local has_non_default_widths = false

    for i, colspec in ipairs(colspecs) do
        -- ColSpec is a pair: [1] = alignment, [2] = width
        local align = colspec[1]
        local width = colspec[2]

        table.insert(aligns, alignment_to_char(align))

        -- Width is a number (0.0-1.0) or ColWidthDefault
        if type(width) == "number" and width > 0 then
            table.insert(widths, tostring(width))
            has_non_default_widths = true
        else
            -- ColWidthDefault or 0
            table.insert(widths, "1")
        end
    end

    -- Only add aligns if there are non-default alignments
    local has_non_default_aligns = false
    for _, a in ipairs(aligns) do
        if a ~= 'd' then
            has_non_default_aligns = true
            break
        end
    end

    if has_non_default_aligns then
        div_attr.attributes.aligns = table.concat(aligns, ',')
    end

    if has_non_default_widths then
        div_attr.attributes.widths = table.concat(widths, ',')
    end

    -- Build div content
    local content = {}

    -- Add caption if present
    if caption and caption.long and #caption.long > 0 then
        for _, block in ipairs(caption.long) do
            table.insert(content, block)
        end
    end

    -- Build list of rows (each row is a list item containing a bullet list of cells)
    local row_items = {}

    -- Add header rows
    for _, row in ipairs(thead_rows) do
        local cells = row.cells
        local cell_blocks_list = {}
        for _, cell in ipairs(cells) do
            table.insert(cell_blocks_list, cell_to_blocks(cell))
        end
        -- Each row item contains a single bullet list of cells
        table.insert(row_items, {pandoc.BulletList(cell_blocks_list)})
    end

    -- Add body rows from all table bodies
    for _, tbody in ipairs(tbodies) do
        for _, row in ipairs(tbody.body) do
            local cells = row.cells
            local cell_blocks_list = {}
            for _, cell in ipairs(cells) do
                table.insert(cell_blocks_list, cell_to_blocks(cell))
            end
            -- Each row item contains a single bullet list of cells
            table.insert(row_items, {pandoc.BulletList(cell_blocks_list)})
        end
    end

    -- Add footer rows if any
    for _, row in ipairs(tfoot.rows) do
        local cells = row.cells
        local cell_blocks_list = {}
        for _, cell in ipairs(cells) do
            table.insert(cell_blocks_list, cell_to_blocks(cell))
        end
        table.insert(row_items, {pandoc.BulletList(cell_blocks_list)})
    end

    -- Create the outer bullet list (list of rows)
    table.insert(content, pandoc.BulletList(row_items))

    return pandoc.Div(content, div_attr)
end

return {{Table = table_to_list_table}}
