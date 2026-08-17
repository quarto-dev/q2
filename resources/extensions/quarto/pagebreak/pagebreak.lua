-- pagebreak.lua
-- Ported from Quarto 1's handlePagebreak
-- (quarto-cli src/resources/filters/quarto-pre/shortcodes-handlers.lua)

local payloads = {
  epub = '<p style="page-break-after: always;"> </p>',
  html = '<div style="page-break-after: always;"></div>',
  latex = '\\newpage{}',
  ooxml = '<w:p><w:r><w:br w:type="page"/></w:r></w:p>',
  odt = '<text:p text:style-name="Pagebreak"/>',
  context = '\\page',
  typst = '#pagebreak()'
}

return {
  pagebreak = function()
    if FORMAT == 'docx' then
      return pandoc.RawBlock('openxml', payloads.ooxml)
    elseif FORMAT == 'pptx' then
      return pandoc.Blocks({})
    elseif FORMAT:match('latex') or FORMAT == 'pdf' then
      return pandoc.RawBlock('tex', payloads.latex)
    elseif FORMAT:match('odt') then
      return pandoc.RawBlock('opendocument', payloads.odt)
    elseif FORMAT == 'typst' then
      return pandoc.RawBlock('typst', payloads.typst)
    elseif FORMAT:match('html.*') or FORMAT:match('revealjs') then
      return pandoc.RawBlock('html', payloads.html)
    elseif FORMAT:match('epub') then
      return pandoc.RawBlock('html', payloads.epub)
    elseif FORMAT:match('context') then
      return pandoc.RawBlock('context', payloads.context)
    else
      -- fall back to a form feed character
      return pandoc.Para(pandoc.Inlines({ pandoc.Str('\f') }))
    end
  end
}
