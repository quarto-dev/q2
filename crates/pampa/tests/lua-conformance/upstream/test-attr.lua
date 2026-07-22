local tasty = require 'tasty'

local test = tasty.test_case
local group = tasty.test_group
local assert = tasty.assert

return {
  group 'Attr' {
    group 'Constructor' {
      test('Attr is a function', function ()
        assert.are_equal(type(Attr), 'function')
      end),
      test('returns null-Attr if no arguments are given', function ()
        local attr = Attr()
        assert.are_equal(attr.identifier, '')
        assert.are_same(attr.classes, {})
        assert.are_same(#attr.attributes, 0)
      end),
      test(
        'accepts string-indexed table or list of pairs as attributes',
        function ()
          local attributes_list = {{'one', '1'}, {'two', '2'}}
          local attr_from_list = Attr('', {}, attributes_list)

          assert.are_equal(attr_from_list.attributes.one, '1')
          assert.are_equal(attr_from_list.attributes.two, '2')

          local attributes_table = {one = '1', two = '2'}
          local attr_from_table = Attr('', {}, attributes_table)
          assert.are_equal(
            attr_from_table.attributes,
            AttributeList(attributes_table)
          )
          assert.are_equal(attr_from_table.attributes.one, '1')
          assert.are_equal(attr_from_table.attributes.two, '2')
        end
      )
    },
    group 'Properties' {
      test('has t and tag property', function ()
        local attr = Attr('')
        assert.are_equal(attr.t, 'Attr')
        assert.are_equal(attr.tag, 'Attr')
      end),
      test('has field `identifier`', function ()
        local attr = Attr 'test'
        assert.are_equal(attr.identifier, 'test')
      end),
      test('can be modified through field `identifier`', function ()
        local attr = Attr 'test'
        attr.identifier = 'new'
        assert.are_equal(attr, Attr 'new')
      end),
      group 'field classes' {
        test('can be read', function ()
          local attr = Attr('', {'one'})
          assert.are_same(attr.classes, {'one'})
        end),
        test('can be set', function ()
          local attr = Attr()
          attr.classes = {'two'}
          assert.are_equal(attr, Attr('', {'two'}))
        end),
        test('contains a pandoc List', function ()
          assert.are_equal(getmetatable(Attr().classes), List)
        end),
      }
    },
    group 'AttributeList' {
      test('allows access via fields', function ()
        local attributes = Attr('', {}, {{'a', '1'}, {'b', '2'}}).attributes
        assert.are_equal(attributes.a, '1')
        assert.are_equal(attributes.b, '2')
      end),
      test('allows access to pairs via numerical indexing', function ()
        local attributes = Attr('', {}, {{'a', '1'}, {'b', '2'}}).attributes
        assert.are_same(attributes[1], {'a', '1'})
        assert.are_same(attributes[2], {'b', '2'})
      end),
      test('allows replacing a pair', function ()
        local attributes = AttributeList{{'a', '1'}, {'b', '2'}}
        attributes[1] = {'t','five'}
        assert.are_same(attributes[1], {'t', 'five'})
        assert.are_same(attributes[2], {'b', '2'})
      end),
      test('allows to remove a pair', function ()
         local attributes = AttributeList{{'a', '1'}, {'b', '2'}}
         attributes[1] = nil
         assert.are_equal(#attributes, 1)
      end),
      test('adds entries by field name', function ()
        local attributes = Attr('',{}, {{'c', '1'}, {'d', '2'}}).attributes
        attributes.e = '3'
        assert.are_same(
          attributes,
          -- checking the full AttributeList would "duplicate" entries
          AttributeList{{'c', '1'}, {'d', '2'}, {'e', '3'}}
        )
      end),
      test('deletes entries by field name', function ()
        local attributes = Attr('',{}, {a = '1', b = '2'}).attributes
        attributes.a = nil
        assert.is_nil(attributes.a)
        assert.are_same(attributes, AttributeList{{'b', '2'}})
      end),
      test('remains unchanged if deleted key did not exist', function ()
        local assoc_list = List:new {{'alpha', 'x'}, {'beta', 'y'}}
        local attributes = Attr('', {}, assoc_list).attributes
        attributes.a = nil
        local new_assoc_list = List()
        for k, v in pairs(attributes) do
          new_assoc_list:insert({k, v})
        end
        assert.are_same(new_assoc_list, assoc_list)
      end),
      test('gives key-value pairs when iterated-over', function ()
        local attributes = {width = '11', height = '22', name = 'test'}
        local attr = Attr('', {}, attributes)
        local count = 0
        for k, v in pairs(attr.attributes) do
          assert.are_equal(attributes[k], v)
          count = count + 1
        end
        assert.are_equal(count, 3)
      end)
    },
    group 'HTML-like attribute tables' {
      test('in element constructor', function ()
        local html_attributes = {
          id = 'the-id',
          class = 'class1 class2',
          width = '11',
          height = '12'
        }
        local attr = Attr(html_attributes)
        assert.are_equal(attr.identifier, 'the-id')
        assert.are_equal(attr.classes[1], 'class1')
        assert.are_equal(attr.classes[2], 'class2')
        assert.are_equal(attr.attributes.width, '11')
        assert.are_equal(attr.attributes.height, '12')
      end),
    }
  }
}
