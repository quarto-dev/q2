# docs-lookup

prompt: Use only the Quarto Hub MCP tools (no shell, no local filesystem). A teammate asks: "I renamed a page in my Quarto website and now old links break — how do I keep the old URL working?" Find the answer in the documentation the server exposes (not the web), then reply with the answer in one sentence AND the href of the documentation page you found it on.

# init: model=claude-fable-5-1 tools=50

**assistant:** I'll load the Quarto Hub `docs` tool and search the embedded Quarto 2 documentation for how to keep an old URL working after a page rename.

**tool_use ToolSearch:** {"query":"select:mcp__quarto-hub__docs"}
**tool_result:** [{"type":"tool_reference","tool_name":"mcp__quarto-hub__docs"}]

**tool_use mcp__quarto-hub__docs:** {"query":"aliases redirect old URL renamed page","max_results":8}
**tool_result:** {"results":[{"href":"guides/projects/aliases.md","title":"Aliases: keeping old URLs working","snippet":"# Aliases: keeping old URLs working"},{"href":"errors/project/Q-5-24.md","title":"Two Pages Claim the Same Alias","snippet":"- /deploy.html      # same old URL, different page"},{"href":"errors/pr…

**tool_use mcp__quarto-hub__docs:** {"query":"redirects","max_results":8}
**tool_result:** {"results":[{"href":"guides/projects/aliases.md","title":"Aliases: keeping old URLs working","snippet":"A page marked `draft: true` writes no redirects. A live URL pointing at"},{"href":"errors/project/Q-5-25.md","title":"Alias Paths Differ Only by Case","snippet":"On a case-insensitive filesystem t…

**assistant:** The top hit is a dedicated guide page. I'll read it to confirm the exact mechanism.

**tool_use mcp__quarto-hub__docs:** {"page":"guides/projects/aliases.md"}
**tool_result:** {"href":"guides/projects/aliases.md","title":"Aliases: keeping old URLs working","markdown":"# Aliases: keeping old URLs working\n\n## Overview\n\nWhen a page moves or gets renamed, every link, bookmark and search\nresult pointing at its old URL breaks. The `aliases:` key keeps those\nURLs working: …

**assistant:** Add an `aliases:` list to the renamed page's front matter with the old URL path (for example `aliases: [/old-deploy-guide.html]`), and Quarto writes a redirect file at that old location so the link keeps working (website projects only).

Documentation page: `guides/projects/aliases.md` (titled "Aliases: keeping old URLs working").

# result: subtype=success is_error=false turns=5 cost=$0.9305

Add an `aliases:` list to the renamed page's front matter with the old URL path (for example `aliases: [/old-deploy-guide.html]`), and Quarto writes a redirect file at that old location so the link keeps working (website projects only).

Documentation page: `guides/projects/aliases.md` (titled "Aliases: keeping old URLs working").
