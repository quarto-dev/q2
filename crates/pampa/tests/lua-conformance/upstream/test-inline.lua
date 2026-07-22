local tasty = require 'tasty'

local test = tasty.test_case
local group = tasty.test_group
local assert = tasty.assert

return {
  group "Inline" {
    group 'Cite' {
      test('has property `content`', function ()
        local cite = Cite({Emph 'important'}, {})
        assert.are_same(cite.content, {Emph {Str 'important'}})

        cite.content = 'boring'
        assert.are_equal(cite, Cite({Str 'boring'}, {}))
      end),
      test('has list of citations in property `cite`', function ()
        local citations = {
          Citation('einstein1905', 'NormalCitation')
        }
        local cite = Cite('relativity', citations)
        assert.are_same(cite.citations, citations)

        local new_citations = {
          citations[1],
          Citation('Poincaré1905', 'NormalCitation')
        }
        cite.citations = new_citations
        assert.are_equal(cite, Cite({'relativity'}, new_citations))
      end),
    },
    group 'Code' {
      test('has property `attr`', function ()
        local code = Code('true', {id='true', foo='bar'})
        assert.are_equal(code.attr, Attr('true', {}, {{'foo', 'bar'}}))

        code.attr = {id='t', fubar='quux'}
        assert.are_equal(
          Code('true', Attr('t', {}, {{'fubar', 'quux'}})),
          code
        )
      end),
      test('has property `text`', function ()
        local code = Code('true')
        assert.are_equal(code.text, 'true')

        code.text = '1 + 1'
        assert.are_equal(Code('1 + 1'), code)
      end),
    },
    group 'Emph' {
      test('has property `content`', function ()
        local elem = Emph{'two', Space(), 'words'}
        assert.are_same(
          elem.content,
          {Str 'two', Space(), Str 'words'}
        )
        elem.content = {'word'}
        assert.are_equal(elem, Emph{'word'})
      end)
    },
    group 'Image' {
      test('has property `caption` of type Inlines', function ()
        local img = Image('example', 'a.png')
        assert.are_same(img.caption, {Str 'example'})

        img.caption = 'A'
        assert.are_equal(img, Image({'A'}, 'a.png'))
        assert.are_equal(
          Image('example', 'a.png').caption,
          Inlines('example')
        )
      end),
      test('has property `src`', function ()
        local img = Image('example', 'sample.png')
        assert.are_same(img.src, 'sample.png')

        img.src = 'example.svg'
        assert.are_equal(img, Image('example', 'example.svg'))
      end),
      test('has property `title`', function ()
        local img = Image('here', 'img.gif', 'example')
        assert.are_same(img.title, 'example')

        img.title = 'a'
        assert.are_equal(img, Image('here', 'img.gif', 'a'))
      end),
      test('has property `attr`', function ()
        local img = Image('up', 'upwards.png', '', {'up', {'point'}})
        assert.are_same(img.attr, Attr {'up', {'point'}})

        img.attr = Attr {'up', {'point', 'button'}}
        assert.are_equal(
          Image('up', 'upwards.png', nil, {'up', {'point', 'button'}}),
          img
        )
      end)
    },
    group 'Link' {
      test('has property `content`', function ()
        local link = Link('example', 'https://example.org')
        assert.are_same(link.content, {Str 'example'})

        link.content = 'commercial'
        link.target = 'https://example.com'
        assert.are_equal(link, Link('commercial', 'https://example.com'))
      end),
      test('has property `target`', function ()
        local link = Link('example', 'https://example.org')
        assert.are_same(link.content, {Str 'example'})

        link.target = 'https://example.com'
        assert.are_equal(link, Link('example', 'https://example.com'))
      end),
      test('has property `title`', function ()
        local link = Link('here', 'https://example.org', 'example')
        assert.are_same(link.title, 'example')

        link.title = 'a'
        assert.are_equal(link, Link('here', 'https://example.org', 'a'))
      end),
      test('has property `attr`', function ()
        local link = Link('up', '../index.html', '', {'up', {'nav'}})
        assert.are_same(link.attr, Attr {'up', {'nav'}})

        link.attr = Attr {'up', {'nav', 'button'}}
        assert.are_equal(
          Link('up', '../index.html', nil, {'up', {'nav', 'button'}}),
          link
        )
      end)
    },
    group 'Math' {
      test('has property `text`', function ()
        local elem = Math(InlineMath, 'x^2')
        assert.are_same(elem.text, 'x^2')
        elem.text = 'a + b'
        assert.are_equal(elem, Math(InlineMath, 'a + b'))
      end),
      test('has property `mathtype`', function ()
        local elem = Math(InlineMath, 'x^2')
        assert.are_same(elem.mathtype, 'InlineMath')
        elem.mathtype = DisplayMath
        assert.are_equal(elem, Math(DisplayMath, 'x^2'))
      end),
    },
    group 'Note' {
      test('has property `content`', function ()
        local elem = Note{Para {'two', Space(), 'words'}}
        assert.are_same(
          elem.content,
          {Para {Str 'two', Space(), Str 'words'}}
        )
        elem.content = Plain 'word'
        assert.are_equal(elem, Note{'word'})
      end)
    },
    group 'Quoted' {
      test('has property `content`', function ()
        local elem = Quoted('SingleQuote', Emph{'emph'})
        assert.are_same(
          elem.content,
          {Emph{Str 'emph'}}
        )
        elem.content = {'word'}
        assert.are_equal(elem, Quoted(SingleQuote, {'word'}))
      end),
      test('has property `quotetype`', function ()
        local elem = Quoted('SingleQuote', 'a')
        assert.are_same(elem.quotetype, SingleQuote)
        elem.quotetype = 'DoubleQuote'
        assert.are_equal(elem, Quoted(DoubleQuote, {'a'}))
      end)
    },
    group 'RawInline' {
      test('has property `format`', function ()
        local elem = RawInline('html', '<mark>nice</mark>')
        assert.are_same(elem.format, 'html')
        elem.format = 'html5'
        assert.are_equal(elem, RawInline('html5', '<mark>nice</mark>'))
      end),
      test('has property `text`', function ()
        local elem = RawInline('html', '<mark>nice</mark>')
        assert.are_same(elem.text, '<mark>nice</mark>')
        elem.text = '<var>x</var>'
        assert.are_equal(elem, RawInline('html', '<var>x</var>'))
      end)
    },
    group 'SmallCaps' {
      test('has property `content`', function ()
        local elem = SmallCaps{'two', Space(), 'words'}
        assert.are_same(
          elem.content,
          {Str 'two', Space(), Str 'words'}
        )
        elem.content = {'word'}
        assert.are_equal(elem, SmallCaps{'word'})
      end)
    },
    group 'SoftBreak' {
      test('can be constructed', function ()
        local sb = SoftBreak()
        assert.are_equal(sb.t, 'SoftBreak')
      end)
    },
    group 'Span' {
      test('has property `attr`', function ()
        local elem = Span('one', {'', {'number'}})
        assert.are_same(
          elem.attr,
          Attr('', {'number'})
        )
        elem.attr = {'', {}, {{'a', 'b'}}}
        assert.are_equal(elem, Span({'one'}, {a='b'}))
      end),
      test('has property `content`', function ()
        local elem = Span{'two', Space(), 'words'}
        assert.are_same(
          elem.content,
          {Str 'two', Space(), Str 'words'}
        )
        elem.content = {'word'}
        assert.are_equal(elem, Span{'word'})
      end)
    },
    group 'Str' {
      test('has property `text`', function ()
        local elem = Str 'nein'
        assert.are_same(elem.text, 'nein')
        elem.text = 'doch'
        assert.are_equal(elem, Str 'doch')
      end)
    },
    group 'Strikeout' {
      test('has property `content`', function ()
        local elem = Strikeout{'two', Space(), 'words'}
        assert.are_same(
          elem.content,
          {Str 'two', Space(), Str 'words'}
        )
        elem.content = {'word'}
        assert.are_equal(elem, Strikeout{'word'})
      end)
    },
    group 'Strong' {
      test('has property `content`', function ()
        local elem = Strong{'two', Space(), 'words'}
        assert.are_same(
          elem.content,
          {Str 'two', Space(), Str 'words'}
        )
        elem.content = {'word'}
        assert.are_equal(elem, Strong{'word'})
      end)
    },
    group 'Subscript' {
      test('has property `content`', function ()
        local elem = Subscript{'two', Space(), 'words'}
        assert.are_same(
          elem.content,
          {Str 'two', Space(), Str 'words'}
        )
        elem.content = {'word'}
        assert.are_equal(elem, Subscript{'word'})
      end)
    },
    group 'Superscript' {
      test('has property `content`', function ()
        local elem = Superscript{'two', Space(), 'words'}
        assert.are_same(
          elem.content,
          {Str 'two', Space(), Str 'words'}
        )
        elem.content = {'word'}
        assert.are_equal(elem, Superscript{'word'})
      end)
    },
    group 'Underline' {
      test('has property `content`', function ()
        local elem = Underline{'two', Space(), 'words'}
        assert.are_same(
          elem.content,
          {Str 'two', Space(), Str 'words'}
        )
        elem.content = {'word'}
        assert.are_equal(elem, Underline{'word'})
      end)
    },
  },
  group "Inlines" {
    group 'Constructor' {
      test('splits a string into words', function ()
        assert.are_same(
          Inlines 'Absolute Giganten',
          {Str 'Absolute', Space(), Str 'Giganten'}
        )
      end),
      test('converts single Inline into List', function ()
        assert.are_same(
          Inlines(Emph{Str'Important'}),
          {Emph{Str'Important'}}
        )
      end),
      test('converts elements in a list into Inlines', function ()
        assert.are_same(
          Inlines{'Molecular', Space(), 'Biology'},
          {Str 'Molecular', Space(), Str 'Biology'}
        )
      end),
      test('tabs are treated as space', function ()
        local expected = {
          Str 'Linkin', Space(), Str 'Park', Space(),
          Str '-', Space(), Str 'Papercut'
        }
        assert.are_same(Inlines('Linkin Park\t-\tPapercut'), expected)
      end),
      test('newlines are treated as softbreaks', function ()
        local expected = {
          Str 'Porcupine', Space(), Str 'Tree',
          SoftBreak(), Str '-', SoftBreak(),
          Str 'Blackest',  Space(), Str 'Eyes'
        }
        assert.are_same(
          Inlines('Porcupine Tree\n-\nBlackest Eyes'),
          expected
        )
      end),
      test('can be mapped over', function ()
        local words = Inlines 'good idea'
        assert.are_same(
          words:map(function (x) return x.t end),
          {'Str', 'Space', 'Str'}
        )
      end),
      test('gives sensible error message', function ()
        assert.error_matches(
          function() Inlines(nil) end,
          "Inline, list of Inlines, or string"
        )
      end)
    },
    group 'clone' {
      test('function exists', function ()
        assert.are_equal(type(Inlines({}).clone), 'function')
      end),
      test('clones the list', function ()
        local inlns = Inlines{'Hello,', Space(), 'World!'}
        assert.are_same(inlns, inlns:clone())
      end),
      test('deep-clones the list', function ()
        local inlns = Inlines{Str 'Hello,', Space(), Str 'World!'}
        local copy = inlns:clone()
        copy[1].text = 'Bonjour,'
        assert.are_same(Inlines{Str 'Bonjour,', Space(), Str 'World!'}, copy)
        assert.are_same(Inlines{Str 'Hello,', Space(), Str 'World!'}, inlns)
      end)
    },
    group 'tostring' {
      test('works on an empty list', function ()
        assert.are_equal(
          tostring(Inlines{}),
          '[]'
        )
      end),
      test('simple inlines', function ()
        assert.are_equal(
          tostring(Inlines 'Bonjour, Monsieur !'),
          '[Str "Bonjour,",Space,Str "Monsieur",Space,Str "!"]'
        )
      end),
    },
    group 'walk' {
      test('modifies Inline subelements', function ()
        assert.are_same(
          Inlines 'Hello, Jake!',
          (Inlines 'Hello, World!'):walk{
            Str = function (str)
              return str.text == 'World!' and Str('Jake!') or nil
            end
          }
        )
      end),
    }
  },
  group 'walk' {
    test('modifies Inline subelements', function ()
      local span = Span 'Hello, World!'
      local expected = Span 'Hello, John!'
      assert.are_equal(
        expected,
        span:walk{
          Str = function (str)
            return str.text == 'World!' and Str('John!') or nil
          end
        }
      )
    end),
    test('applies filter only on subtree', function ()
      local str = Str 'Hello'
      assert.are_equal(
        Str 'Hello',
        str:walk{
          Str = function (str)
            return str.text == 'Hello' and Str('Goodbye') or nil
          end
        }
      )
    end),
    test('modifies blocks in notes', function ()
      local note = Note{Para 'The proof is trivial.'}
      assert.are_equal(
        Note{Plain 'The proof is trivial.'},
        note:walk{
          Para = function (para)
            return Plain(para.content)
          end
        }
      )
    end),
    test('uses `Inlines` for lists of inlines', function ()
      local span = Span{Emph 'Kid A'}
      assert.are_equal(
        Span{Emph 'Kid A+'},
        span:walk{
          Inlines = function (inlns)
            if Span(inlns) == Span 'Kid A' then
              return Inlines 'Kid A+'
            end
          end
        }
      )
    end),
    test('handles inline elements before inline lists', function ()
      local span = Span{Emph 'Red door'}
      assert.are_equal(
        Span{Emph 'Paint it Black'},
        span:walk{
          Inlines = function (inlns)
            if Span(inlns) == Span('Paint it') then
              return inlns .. {Space(), 'Black'}
            end
          end,
          Str = function (str)
            if str == Str 'Red' then
              return 'Paint'
            elseif str == Str 'door' then
              return 'it'
            end
          end
        }
      )
    end),
    test('uses order Inline -> Inlines -> Block -> Blocks', function ()
      local names = List{}
      Note{Para 'Human After All', CodeBlock 'Alive 2007'}:walk{
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
      assert.are_equal(
        'Str, Space, Str, Space, Str, Inlines, Para, CodeBlock, Blocks',
        table.concat(names, ', ')
      )
    end),
  },

  group 'marshalling' {
    test('bare strings become Str values', function ()
      assert.are_equal(Inlines{'a'}, Inlines{Str 'a'})
    end),

    group '__toinline metamethod' {
      test('metamethod __toinline is called when available', function ()
        local function toinline (t)
          return Code(t.code, {id = t.id, class = t.class})
        end
        local my_code = setmetatable(
          {code = 'open access', id='opn'},
          {__toinline = toinline}
        )
        assert.are_equal(
          Inlines{Code('open access', {'opn'})},
          Inlines{my_code}
        )
      end),

      test("metafield is ignored if it's not a function", function ()
        local bad_inline = setmetatable({'a'}, {__toinline = true})
        assert.are_equal(
          Inlines({'a'}),
          Inlines(bad_inline)
        )
      end),

      test("non-Inline return values are ignored", function ()
        local function toinline ()
          return "not an inline"
        end
        local bad_inline = setmetatable({'a'}, {__toinline = toinline})
        assert.are_equal(
          Inlines({'a'}),
          Inlines(bad_inline)
        )
      end),

      test("objects can be used as singleton lists", function ()
        local function toinline (t)
          return Code(t.code, {id = t.id, class = t.class})
        end
        local my_code = setmetatable(
          {code = 'open access', id='opn'},
          {__toinline = toinline}
        )
        assert.are_equal(
          Inlines(Code('open access', {'opn'})),
          Inlines(my_code)
        )
      end)
    }
  }
}
