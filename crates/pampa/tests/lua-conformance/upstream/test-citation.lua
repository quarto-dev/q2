--
-- Tests for the pandoc types module
--
local tasty = require 'tasty'

local group = tasty.test_group
local test = tasty.test_case
local assert = tasty.assert

return {
  group 'Citation' {
    test('can be cloned', function ()
      local cit = Citation('leibniz', AuthorInText)
      local cloned = cit:clone()
      cit.id = 'newton'
      assert.are_same(cloned.id, 'leibniz')
      assert.are_same(cit.id, 'newton')
      assert.are_same(cit.mode, cloned.mode)
    end),
    group 'field `id`' {
      test('can be read', function ()
        assert.are_equal(
          Citation('einstein1905', 'NormalCitation').id,
          'einstein1905'
        )
      end),
      test('can be set', function ()
        local c = Citation('einstein1905', 'NormalCitation')
        c.id = 'Poincaré1905'
        assert.are_equal(c, Citation('Poincaré1905', 'NormalCitation'))
      end)
    },
    group 'field `mode`' {
      test('can be read', function ()
        assert.are_equal(
          Citation('einstein1905', 'NormalCitation').mode,
          'NormalCitation'
        )
      end),
      test('can be set', function ()
        local c = Citation('Poincaré1905', 'NormalCitation')
        c.mode = 'AuthorInText'
        assert.are_equal(c, Citation('Poincaré1905', 'AuthorInText'))
      end)
    },
    group 'field `prefix`' {
      test('can be read', function ()
        assert.are_same(
          Citation('einstein1905', 'NormalCitation', {'x'}).prefix,
          {Str 'x'}
        )
      end),
      test('can be set', function ()
        local c = Citation('Poincaré1905', 'NormalCitation')
        c.prefix = {'y'}
        assert.are_equal(
          c,
          Citation('Poincaré1905', 'NormalCitation', {'y'})
        )
      end),
    },
    group 'field `suffix`' {
      test('can be read', function ()
        assert.are_same(
          Citation('einstein1905', 'NormalCitation', {}, 'is great').suffix,
          {Str 'is', Space(), Str 'great'}
        )
      end),
      test('can be set', function ()
        local c = Citation('Poincaré1905', 'NormalCitation')
        c.suffix = {'why'}
        assert.are_equal(
          c,
          Citation('Poincaré1905', 'NormalCitation', {}, {'why'})
        )
      end),
    },
    group 'field `note_num`' {
      test('can be read', function ()
        assert.are_equal(
          Citation('einstein1905', 'NormalCitation', {}, {}, 7).note_num,
          7
        )
      end),
      test('can be set', function ()
        local c = Citation('Poincaré1905', 'NormalCitation')
        c.note_num = 23
        assert.are_equal(
          c,
          Citation('Poincaré1905', 'NormalCitation', {}, {}, 23)
        )
      end),
    },
    group 'field `hash`' {
      test('can be read', function ()
        assert.are_equal(
          Citation('einstein1905', 'NormalCitation', {}, {}, 0, 5).hash,
          5
        )
      end),
      test('can be set', function ()
        local c = Citation('Poincaré1905', 'NormalCitation')
        c.hash = 23
        assert.are_equal(
          c,
          Citation('Poincaré1905', 'NormalCitation', {}, {}, 0, 23)
        )
      end)
    }
  }
}
