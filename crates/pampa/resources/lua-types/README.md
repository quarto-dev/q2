# Lua Type Annotations for quarto-markdown-pandoc

This directory contains [LuaLS](https://luals.github.io/) (Lua Language Server) type annotation files that provide IDE support for writing Lua filters.

## Features

When configured, these annotations provide:

- **Autocomplete** for `pandoc.*` constructors and `quarto.*` functions
- **Type checking** for function arguments and return values
- **Inline documentation** when hovering over functions
- **Go to definition** support

## Supported API

These annotations document **only** the API that `quarto-markdown-pandoc` actually implements, which is a subset of the full Pandoc Lua API. Using functions not documented here may result in runtime errors.

### pandoc namespace
- Element constructors: `pandoc.Str()`, `pandoc.Para()`, `pandoc.Link()`, etc.
- List types: `pandoc.List`, `pandoc.Inlines`, `pandoc.Blocks`
- Utilities: `pandoc.utils.stringify()`

### quarto namespace
- `quarto.warn(message, element?)` - Emit a warning diagnostic
- `quarto.error(message, element?)` - Emit an error diagnostic

### Global variables
- `FORMAT` - Target output format (e.g., "html", "latex")
- `PANDOC_VERSION` - Version table `{major, minor, patch}`
- `PANDOC_API_VERSION` - API version table
- `PANDOC_SCRIPT_FILE` - Path to the current filter script

## Configuration

### VS Code with sumneko.lua extension

1. Install the [Lua extension](https://marketplace.visualstudio.com/items?itemName=sumneko.lua) by sumneko

2. Add to your VS Code settings (`.vscode/settings.json` or user settings):

```json
{
  "Lua.workspace.library": [
    "/path/to/quarto-markdown-pandoc/resources/lua-types"
  ],
  "Lua.runtime.version": "Lua 5.4",
  "Lua.diagnostics.globals": [
    "FORMAT",
    "PANDOC_VERSION",
    "PANDOC_API_VERSION",
    "PANDOC_SCRIPT_FILE"
  ]
}
```

Replace `/path/to/quarto-markdown-pandoc` with the actual path to this repository.

### Neovim with nvim-lspconfig

Add to your Neovim Lua configuration:

```lua
require('lspconfig').lua_ls.setup {
  settings = {
    Lua = {
      runtime = {
        version = 'Lua 5.4',
      },
      workspace = {
        library = {
          '/path/to/quarto-markdown-pandoc/resources/lua-types',
        },
      },
      diagnostics = {
        globals = {
          'FORMAT',
          'PANDOC_VERSION',
          'PANDOC_API_VERSION',
          'PANDOC_SCRIPT_FILE',
        },
      },
    },
  },
}
```

### Project-specific configuration

You can also create a `.luarc.json` file in your project root:

```json
{
  "$schema": "https://raw.githubusercontent.com/sumneko/vscode-lua/master/setting/schema.json",
  "runtime.version": "Lua 5.4",
  "workspace.library": [
    "/path/to/quarto-markdown-pandoc/resources/lua-types"
  ],
  "diagnostics.globals": [
    "FORMAT",
    "PANDOC_VERSION",
    "PANDOC_API_VERSION",
    "PANDOC_SCRIPT_FILE"
  ]
}
```

## File Structure

```
lua-types/
├── README.md           # This file
├── pandoc/
│   ├── pandoc.lua      # Main pandoc module and constructors
│   ├── global.lua      # Global variables (FORMAT, etc.)
│   ├── inlines.lua     # Inline element types
│   ├── blocks.lua      # Block element types
│   ├── components.lua  # Attr, Citation, Caption, etc.
│   ├── List.lua        # pandoc.List methods
│   └── utils.lua       # pandoc.utils module
└── quarto/
    ├── quarto.lua      # Main quarto module
    └── diagnostics.lua # quarto.warn(), quarto.error()
```

## Contributing

When adding new Lua API support to `quarto-markdown-pandoc`, please update the corresponding type annotation files to keep the documentation in sync with the implementation.

## References

- [LuaLS Annotations Documentation](https://luals.github.io/wiki/annotations/)
- [Pandoc Lua Filters Documentation](https://pandoc.org/lua-filters.html)
