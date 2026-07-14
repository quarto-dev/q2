local tasty = require 'tasty'

local test = tasty.test_case
local group = tasty.test_group
local assert = tasty.assert

return {
  group "Cell" {
    group 'Constructor' {
      test('align defaults to `AlignDefault`', function ()
        local cell = Cell({})
        assert.are_equal(cell.alignment, AlignDefault)
      end),
      test('row span defaults to 1', function ()
        local cell = Cell{}
        assert.are_equal(cell.row_span, 1)
      end),
      test('col span defaults to 1', function ()
        local cell = Cell{}
        assert.are_equal(cell.col_span, 1)
      end),
      test('attr defaults to null Attr', function ()
        local cell = Cell{}
        assert.are_equal(cell.attr, Attr())
      end),
    },
    group 'properties' {
      test('can modify contents', function ()
        local cell = Cell{}
        cell.contents = {Plain 'snow'}
        assert.are_equal(Cell('snow'), cell)
      end),
      test('modify alignment', function ()
        local cell = Cell({}, 'AlignLeft')
        cell.alignment = 'AlignRight'
        assert.are_equal(Cell({}, 'AlignRight'), cell)
      end),
      test('modify row_span', function ()
        local cell = Cell({}, nil, 4)
        cell.row_span = 2
        assert.are_equal(Cell({}, nil, 2), cell)
      end),
      test('modify col_span', function ()
        local cell = Cell({}, nil, nil, 2)
        cell.col_span = 3
        assert.are_equal(Cell({}, nil, nil, 3), cell)
      end),
      test('modify attr', function ()
        local cell = Cell({}, nil, nil, nil, Attr('before'))
        cell.attr = Attr('after')
        assert.are_equal(Cell({}, nil, nil, nil, Attr('after')), cell)
      end),
    },
    group 'aliases' {
      test('identifier', function ()
        local cell = Cell{}
        cell.identifier = 'yep'
        assert.are_same(Cell({}, nil, nil, nil, 'yep'), cell)
      end),
      test('classes', function ()
        local cell = Cell{}
        cell.classes = {'java'}
        assert.are_same(Cell({}, nil, nil, nil, {'', {'java'}}), cell)
      end),
      test('attributes', function ()
        local cell = Cell{}
        cell.attributes.precipitation = 'snow'
        assert.are_same(Cell({}, nil, nil, nil, {precipitation='snow'}), cell)
      end),
    },
    group 'walk' {
      test('modifies Inline subelements', function ()
        local cell = Cell{Para 'Hello, World!'}
        assert.are_same(
          Cell{Para 'Hello, Jake!'},
          cell:walk{
            Str = function (str)
              return str.text == 'World!' and Str('Jake!') or nil
            end
          }
        )
      end),
      test('uses `Inlines` for lists of inlines', function ()
        local cell = Cell{Emph 'Kid A'}
        assert.are_equal(
          Cell{Emph 'Kid A+'},
          cell:walk{
            Inlines = function (inlns)
              if Span(inlns) == Span 'Kid A' then
                return Span('Kid A+').content
              end
            end
          }
        )
      end),
      test('uses order Inline -> Inlines -> Block -> Blocks', function ()
        local names = List{}
        Cell{Para 'Discovery', CodeBlock 'Homework'}:walk{
          Blocks = function (_)
            names:insert('Blocks')
          end,
          Block = function (b)
            names:insert(b.t)
          end,
          Inline = function (i)
            names:insert(i.t)
          end,
          Inlines = function (_)
            names:insert('Inlines')
          end,
        }
        assert.are_same(
          {'Str', 'Inlines', 'Para', 'CodeBlock', 'Blocks'},
          names
        )
      end),
    }
  },
}
