local tasty = require 'tasty'

local test = tasty.test_case
local group = tasty.test_group
local assert = tasty.assert

return {
  group 'MetaValue elements' {
    test('MetaList elements behave like lists', function ()
      local metalist = MetaList{}
      assert.are_equal(type(metalist.insert), 'function')
      assert.are_equal(type(metalist.remove), 'function')
    end),
    test('Numbers are treated as strings', function ()
      local metalist = MetaList{5, 23, 13.37}
      assert.are_same(metalist, MetaList{'5', '23', '13.37'})
    end)
  }
}
