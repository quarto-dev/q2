-- Stand-in for the retired quarto-ext/lightbox extension (bd-kcpkgftw).
-- Quarto drops it in favour of the built-in lightbox, so this must never
-- run: if it does, every image gets a `legacy-extension-ran` class.
function Image(img)
  img.classes:insert("legacy-extension-ran")
  return img
end
