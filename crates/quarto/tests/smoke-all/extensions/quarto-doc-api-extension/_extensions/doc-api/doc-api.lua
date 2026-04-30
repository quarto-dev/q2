return {
  ['doc-api'] = function(args)
    quarto.doc.add_html_dependency({
      name = 'doc-api-probe',
      stylesheets = { 'extra.css' },
    })
    quarto.doc.include_text('in-header', '<meta name="doc-api-probe" content="SENTINEL-IN-HEADER">')
    quarto.doc.include_text('after-body', '<!-- SENTINEL-AFTER-BODY -->')
    if quarto.doc.is_format('html') then
      return pandoc.Str('IS-HTML-FORMAT')
    end
    return pandoc.Str('NOT-HTML-FORMAT')
  end
}
