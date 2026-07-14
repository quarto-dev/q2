local tasty = require 'tasty'

local test = tasty.test_case
local group = tasty.test_group
local assert = tasty.assert

return {
  group 'ListAttributes' {
    test('has field `start`', function ()
      local la = ListAttributes(7, DefaultStyle, Period)
      assert.are_equal(la.start, 7)
    end),
    test('has field `style`', function ()
      local la = ListAttributes(1, Example, Period)
      assert.are_equal(la.style, 'Example')
    end),
    test('has field `delimiter`', function ()
      local la = ListAttributes(1, Example, Period)
      assert.are_equal(la.delimiter, 'Period')
    end),
    test('can be compared on equality', function ()
      assert.are_equal(
        ListAttributes(2, DefaultStyle, Period),
        ListAttributes(2, DefaultStyle, Period)
      )
      assert.is_falsy(
        ListAttributes(2, DefaultStyle, Period) ==
        ListAttributes(4, DefaultStyle, Period)
      )
    end),
    test('can be modified through `start`', function ()
      local la = ListAttributes(3, Decimal, OneParen)
      la.start = 20
      assert.are_equal(la, ListAttributes(20, Decimal, OneParen))
    end),
    test('can be modified through `style`', function ()
      local la = ListAttributes(3, Decimal, OneParen)
      la.style = LowerRoman
      assert.are_equal(la, ListAttributes(3, LowerRoman, OneParen))
    end),
    test('can be modified through `delimiter`', function ()
      local la = ListAttributes(5, UpperAlpha, DefaultDelim)
      la.delimiter = TwoParens
      assert.are_equal(la, ListAttributes(5, UpperAlpha, TwoParens))
    end),
    test('can be cloned', function ()
      local la = ListAttributes(2, DefaultStyle, Period)
      local cloned = la:clone()
      assert.are_equal(la, cloned)
      la.start = 9
      assert.are_same(cloned.start, 2)
    end),
    group 'Constructor' {
      test('omitting a start numer sets it to 1', function ()
             assert.are_equal(ListAttributes().start, 1)
      end),
      test('omitting a style sets it to DefaultStyle', function ()
        assert.are_equal(ListAttributes(0).style, DefaultStyle)
      end),
      test('omitting a delimiter sets it to DefaultDelim', function ()
        assert.are_equal(ListAttributes(0, UpperRoman).delimiter, DefaultDelim)
      end)
    }
  },
}
