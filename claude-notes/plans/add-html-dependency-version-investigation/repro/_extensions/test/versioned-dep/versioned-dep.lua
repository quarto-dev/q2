-- Attach an HTML dependency that declares a version, the way a real
-- extension does. Called once per paragraph to show that the warning is
-- emitted per call rather than per distinct dependency.
function Para(el)
  quarto.doc.add_html_dependency({
    name = 'versioned-dep',
    version = '1.0.0',
    scripts = { 'versioned-dep.js' }
  })
  return nil
end
