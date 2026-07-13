local tasty = require 'tasty'

local test = tasty.test_case
local group = tasty.test_group
local assert = tasty.assert

local default_caption = {Str 'Languages', Space(), Str 'overview.'}
local default_aligns = {AlignDefault, AlignDefault}
local default_widths = {0, 0}
local default_headers = {{Plain({Str "Language"})}, {Plain({Str "Typing"})}}
local default_rows = {
  {{Plain "Haskell"}, {Plain "static"}},
  {{Plain "Lua"}, {Plain "Dynamic"}},
}

return {
  group 'SimpleTable' {
    test('can access properties', function ()
      local simple_table = SimpleTable(
        default_caption,
        default_aligns,
        default_widths,
        default_headers,
        default_rows
      )
      assert.are_same(simple_table.caption, default_caption)
      assert.are_same(simple_table.aligns, default_aligns)
      assert.are_same(simple_table.widths, default_widths)
      assert.are_same(simple_table.headers, default_headers)
      assert.are_same(simple_table.rows, default_rows)
    end),
    test('can modify properties', function ()
      local new_table = SimpleTable(
        default_caption,
        default_aligns,
        {0.5, 0.5},
        default_headers,
        default_rows
      )

      new_table.caption = {Str 'Good', Space(),
                           Str 'languages'}
      new_table.aligns[1] = AlignLeft
      new_table.widths = {0, 0}
      new_table.headers[2] = {Plain{Str 'compiled/interpreted'}}
      new_table.rows[1][2] = {Plain{Str 'both'}}
      new_table.rows[2][2] = {Plain{Str 'interpreted'}}

      local expected_table = SimpleTable(
        {Str 'Good', Space(), Str 'languages'},
        {AlignLeft, AlignDefault},
        {0, 0},
        {{Plain 'Language'}, {Plain 'compiled/interpreted'}},
        {
          {{Plain 'Haskell'}, {Plain 'both'}},
          {{Plain 'Lua'}, {Plain 'interpreted'}}
        }
      )
      assert.are_same(expected_table, new_table)
    end)
  },
}
