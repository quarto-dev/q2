local tasty = require 'tasty'

local test = tasty.test_case
local group = tasty.test_group
local assert = tasty.assert

return {
  group "TableBody" {
    group 'Constructor' {
      test('row_head_columns defaults to 0', function ()
        local tbl_body = TableBody{}
        assert.are_equal(tbl_body.row_head_columns, 0)
      end),
      test('attr defaults to null Attr', function ()
        local tbl_body = TableBody{}
        assert.are_equal(tbl_body.attr, Attr())
      end),
      test('Table constructor', function ()
        local old_body = {
          attr={ 'tbl-body' },
          body={ Row{ Cell('body cell') } },
          head={ Row{ Cell('head cell') } },
          row_head_columns=1
        }
        local old_tbl = Table({}, {}, TableHead(), { old_body }, TableFoot())

        local new_body = TableBody(
          { Row{ Cell('body cell') } },
          { Row{ Cell('head cell') } },
          1,
          { 'tbl-body' }
        )
        local new_tbl = Table({}, {}, TableHead(), { new_body }, TableFoot())
        assert.are_equal(old_tbl, new_tbl)
      end),
    },
    group 'properties' {
      test('modify body', function ()
        local tbl_body = TableBody{}
        tbl_body.body = { Row{ Cell('pass') } }
        assert.are_equal(TableBody({ Row{ Cell('pass') } }, {}, 0), tbl_body)
      end),
      test('modify head', function ()
        local tbl_body = TableBody{}
        tbl_body.head = { Row{ Cell('pass') } }
        assert.are_equal(TableBody({}, { Row{ Cell('pass') } }, 0), tbl_body)
      end),
      test('modify row_head_columns', function ()
        local tbl_body = TableBody{}
        tbl_body.row_head_columns = 5
        assert.are_equal(TableBody({}, {}, 5), tbl_body)
      end),
      test('modify attr', function ()
        local tbl_body = TableBody({}, {}, 0, Attr('before'))
        tbl_body.attr = Attr('after')
        assert.are_equal(TableBody({}, {}, 0, Attr('after')), tbl_body)
      end),
    },
    group 'aliases' {
      test('identifier', function ()
        local tbl_body = TableBody{}
        tbl_body.identifier = 'yep'
        assert.are_same(TableBody({}, {}, 0, 'yep'), tbl_body)
      end),
      test('classes', function ()
        local tbl_body = TableBody{}
        tbl_body.classes = {'java'}
        assert.are_same(TableBody({}, {}, 0, {'', {'java'}}), tbl_body)
      end),
      test('attributes', function ()
        local tbl_body = TableBody{}
        tbl_body.attributes.precipitation = 'snow'
        assert.are_same(TableBody({}, {}, 0, {precipitation='snow'}), tbl_body)
      end),
    },
  },
  group "Row" {
    group "Constructor" {
      test('Can be called without arguments', function ()
        assert.are_equal(Row(), Row{})
      end),
    },
    group "unmarshalling" {
      test('A list can be used instead', function ()
        assert.are_equal(
          TableHead{Row({Cell "hi"}, "row-1")},
          TableHead{{"row-1", {Cell "hi"}}}
        )
        assert.are_equal(
          TableHead{Row({Cell "hi"})},
          TableHead{{Cell "hi"}}
        )
        -- assert.are_equal(Row({Cell "hi"}, "row-1"), {"row-1", {Cell "hi"}})
      end),
    },
  }
}
