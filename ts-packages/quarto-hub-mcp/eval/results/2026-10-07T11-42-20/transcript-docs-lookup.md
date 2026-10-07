# docs-lookup

prompt: Use only the Quarto Hub MCP tools (no shell, no local filesystem). A teammate asks: "I renamed a page in my Quarto website and now old links break — how do I keep the old URL working?" Find the answer in the documentation the server exposes (not the web), then reply with the answer in one sentence AND the href of the documentation page you found it on.

# init: model=claude-fable-5-1 tools=51

**assistant:** I'll load the Quarto Hub `docs` tool and search the server's documentation for how to keep an old URL working after a page rename.

**tool_use ToolSearch:** {"query":"select:mcp__quarto-hub__docs","max_results":1}
**tool_result:** [{"type":"tool_reference","tool_name":"mcp__quarto-hub__docs"}]

**tool_use mcp__quarto-hub__docs:** {"query":"aliases redirect old URL renamed page","max_results":8}

**tool_use mcp__quarto-hub__docs:** {"query":"aliases","max_results":8}
**tool_result:** {"results":[{"href":"guides/projects/aliases.md","title":"Aliases: keeping old URLs working","snippet":"# Aliases: keeping old URLs working"},{"href":"errors/project/Q-5-24.md","title":"Two Pages Claim the Same Alias","snippet":"- /deploy.html      # same old URL, different page"},{"href":"errors/pr…
**tool_result:** {"results":[{"href":"guides/projects/aliases.md","title":"Aliases: keeping old URLs working","snippet":"# Aliases: keeping old URLs working"},{"href":"errors/project/Q-5-24.md","title":"Two Pages Claim the Same Alias","snippet":"> Two pages declare `aliases:` entries that resolve to the same"},{"hre…

**assistant:** Both searches point to the same guide. Fetching it to confirm the exact mechanism.

**tool_use mcp__quarto-hub__docs:** {"page":"guides/projects/aliases.md"}
**tool_result:** {"href":"guides/projects/aliases.md","title":"Aliases: keeping old URLs working","markdown":"# Aliases: keeping old URLs working\n\n## Overview\n\nWhen a page moves or gets renamed, every link, bookmark and search\nresult pointing at its old URL breaks. The `aliases:` key keeps those\nURLs working: …

**assistant:** Add an `aliases:` list to the renamed page's front matter containing the old site-relative URL path (for example `aliases: [/old-page.html]`), and Quarto will write a small redirect file at that old location when it renders the website.

Documentation page: `guides/projects/aliases.md` ("Aliases: keeping old URLs working")

Two details from that page worth passing along to your teammate:

- **Leading slash means site root.** An alias like `/guides/deploy.html` is the old URL path in the built si…

# result: subtype=success is_error=false turns=5 cost=$0.5907

Add an `aliases:` list to the renamed page's front matter containing the old site-relative URL path (for example `aliases: [/old-page.html]`), and Quarto will write a small redirect file at that old location when it renders the website.

Documentation page: `guides/projects/aliases.md` ("Aliases: keeping old URLs working")

Two details from that page worth passing along to your teammate:

- **Leading slash means site root.** An alias like `/guides/deploy.html` is the old URL path in the built site, not a source file path. Paths without a leading slash are relative to the page's own output location.
- **Website projects only.** The key has no effect in other project types, and Quarto errors rather than silently skipping if two pages claim the same alias or an alias would overwrite a real page.
