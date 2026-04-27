# Syntax Highlighting Ecosystem Survey: Modern Static-Site Generators

**Scope:** Survey of non-Pandoc/Hugo systems (Shiki, Prism.js, highlight.js, tree-sitter in the wild)
**Focus:** Pipeline location, token taxonomy, HTML output shape, extension mechanisms, TextMate vs tree-sitter rationale
**Date:** 2026-04-19

## Executive Summary

Three distinct architectural families dominate modern syntax highlighting:

1. **TextMate-grammar-based** (Shiki): Build-time, grammar-driven via Oniguruma regex
2. **Regex-based lexers** (Prism.js): Client or server-side, simpler regex, broad language coverage
3. **AST-based with query language** (tree-sitter): Parse-time, semantic query files, context-aware

Shiki (used by VitePress, Astro, Docusaurus v3, Nextra) and tree-sitter (now powering GitHub) are the most strategically important for evaluating your tree-sitter backbone.

---

## 1. Tree-Sitter (GitHub's Approach)

### Pipeline Location
Tree-sitter highlighting happens at **parse time**, using the full syntax tree. GitHub now uses tree-sitter-highlight for code view rendering ([tree-sitter/docs/src/3-syntax-highlighting.md](https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html)).

### Token Taxonomy
Tree-sitter uses a **semantic capture namespace** system, not string-based token types. Captures are defined in `.scm` (Scheme) query files and are dot-separated like `@keyword`, `@function.method`, `@variable.parameter`, `@local.reference`. Scope names deliberately match TextMate/Linguist conventions for compatibility.

Example capture names (from [tree-sitter-ruby highlights.scm](https://github.com/tree-sitter/tree-sitter-ruby/tree/master/queries)):
- `@keyword` / `@keyword.control`
- `@function` / `@function.builtin` / `@function.method`
- `@variable.parameter`
- `@type` / `@type.builtin`
- `@string` / `@comment`

The **locals query** (`locals.scm`) adds semantic tracking: `@local.scope`, `@local.definition`, `@local.reference`. This enables consistent coloring of variables across their uses within a scope—a unique advantage over grammar-based systems (see below).

### HTML Output Shape
Standard: `<pre class='highlight'><span style='color: ...;'>token</span>...</pre>` with inline styles. The three query files (`highlights.scm`, `locals.scm`, `injections.scm`) control all behavior; no special HTML attributes observed.

### Extension Mechanisms
**New languages:** Author a grammar repo with `tree-sitter.json` metadata and `.scm` query files. Registry is decentralized (GitHub org repos).

**Annotations:** The `locals.scm` file creates semantic context. Injection queries (`injections.scm`) allow embedded language support (e.g., JavaScript in HTML `<script>` tags, Ruby in heredocs). No line-number or line-highlight hooks observed in query syntax.

### Why Tree-Sitter?
- **Accuracy:** Parses full AST; no single-line-per-regex limits.
- **Context awareness:** The locals query tracks scopes/definitions automatically. A parameter named `x` and a local variable `x` can be colored consistently.
- **Language injection:** Heredocs, script tags, template languages handled natively.
- **Maintenance:** Grammars are language-agnostic (one repo = one language); highlights maintained alongside grammar.

---

## 2. Shiki (TextMate-Grammar-Based, Used by VitePress/Astro/Docusaurus v3)

### Pipeline Location
Shiki highlights at **build time** (or optionally render time via Node.js). Uses TextMate grammars loaded from existing sources (VS Code Marketplace, etc.) and compiles them to JavaScript via `oniguruma-to-es` or native Oniguruma bindings.

### Token Taxonomy
TextMate scopes are **dot-separated hierarchies** like `source.python`, `keyword.control.python`, `string.quoted.double.python`. These differ from both Pygments token types and tree-sitter captures:

- **Pygments family** (Rouge, Sphinx/Pygments): `kw`, `kd`, `na`, `s`, `c1` (compact, token-type focused)
- **TextMate scopes**: Context-rich; e.g., `source.js > string.quoted.double > constant.escape` tells you this is an escape sequence in a double-quoted string in JavaScript
- **Tree-sitter captures**: Semantic intent (`@keyword`, `@function`), decoupled from text context

Shiki maps TextMate scopes to CSS class names for theming. A theme file defines color rules like `"string.quoted.double": "#ce9178"`.

### HTML Output Shape
Standard: `<pre class="shiki"><code>...<span class="line"><span class="hljs-string">"..."</span></span></code></pre>`. Shiki optionally wraps lines in `<span class="line">` for line-level control, enabling copy-button integrations or line-number CSS hooks.

### Extension Mechanisms
**New languages:** Provide a TextMate grammar (`.json` plist format). No tree-sitter knowledge required; thousands of grammars exist from VS Code ecosystem.

**Annotations:** Limited to theme-level control. Line highlighting, copy buttons typically added post-render via CSS or JavaScript plugins.

### Why TextMate Grammars?
- **Ecosystem size:** Enormous library of grammars (1000+ languages) from VS Code, Sublime, TextMate communities.
- **Oniguruma expressiveness:** Named subexpression calls and recursive patterns enable constructs impossible in simpler regex (e.g., nested balanced parentheses in some contexts).
- **VS Code alignment:** Single grammar source used by VS Code, Sublime, Shiki, and others—reduces maintenance fragmentation.

**Limitation:** TextMate grammars are **line-based**. A single regex cannot span lines. This creates accuracy trade-offs in e.g. multi-line strings, nested constructs. Tree-sitter avoids this entirely.

---

## 3. Prism.js (Regex-Based, Used by Docusaurus v2, Gatsby)

### Pipeline Location
Prism runs **client-side by default** (JavaScript in the browser) but can run **server-side** (Node.js). It scans the DOM for `<code class="language-xyz">` elements and swaps them with highlighted versions.

### Token Taxonomy
Prism uses semantic CSS class names: `.comment`, `.string`, `.keyword`, `.property`, `.function`, etc. These are simple, non-hierarchical, and theme-agnostic. A token is emitted as `(token_type, token_value)` and wrapped in a span with the corresponding class.

### HTML Output Shape
`<pre><code class="language-xyz"><span class="token keyword">if</span>...</code></pre>`. Minimal structure; no line wrapping by default, though plugins can add line numbers or line highlights as post-processing.

### Extension Mechanisms
**New languages:** Write a language definition file (JavaScript object with regex patterns). Requires understanding Prism's regex DSL and known limitations (documented on Prism's failure page).

**Annotations:** Plugin hooks allow intercepting tokens before/after highlighting. Line-number and line-highlight plugins exist as separate modules.

### Why Prism.js?
- **Lightweight:** 2 KB core + 0.3–0.5 KB per language.
- **No dependencies:** Can run entirely client-side or with minimal server-side footprint.
- **Broad compatibility:** Works in older browsers and minimal JavaScript environments.
- **Plugin ecosystem:** Custom extensions (line numbers, copy buttons, diff highlighting) are well-established.

**Trade-off:** Regex-based approach fails on edge cases (documented). Less accurate than tree-sitter for complex syntax.

---

## 4. Highlight.js (Similar to Prism, Less Standardized)

Highlight.js is a close cousin to Prism: regex-based, can run client or server-side, requires manual language file authoring. No major innovations over Prism in pipeline or architecture. Less commonly adopted in modern static-site generators.

---

## 5. Comparative Analysis

| Dimension | Tree-Sitter | Shiki (TextMate) | Prism.js |
|-----------|-------------|------------------|----------|
| **Pipeline** | Parse-time (full AST) | Build-time (grammar engine) | Client/server-side (regex) |
| **Token Taxonomy** | Semantic captures (`@keyword`, `@function`) | Scope hierarchy (`source.lang.type`) | Flat semantic classes (`.keyword`, `.string`) |
| **Context Awareness** | Yes (locals query tracks scopes) | Limited (line-scoped) | No |
| **Accuracy** | Highest (multi-line, nested constructs) | High (ecosystem proven) | Medium (known edge cases) |
| **Language Coverage** | ~100 (curated; GitHub-driven) | ~1000+ (VS Code + community) | ~200+ (community-driven) |
| **Extension Ease** | High (new tree-sitter grammar) | High (adopt existing TextMate grammar) | Medium (regex authoring) |
| **Ecosystem Size** | Small but growing | Large (VS Code backing) | Medium (mature, stable) |
| **Multi-line Regex** | Yes (AST-based) | No (single-line rule limit) | No |

---

## 6. Why TextMate Grammars Remain Relevant

The TextMate grammar ecosystem persists because:

1. **Oniguruma expressiveness:** Named groups and recursive patterns enable accurate lexing for features impossible in simpler regex. Example: balanced bracket matching in some contexts.
2. **Community momentum:** 20+ years of grammar authoring; VS Code, Sublime, Atom all standardize on TextMate format.
3. **Broad adoption:** A single grammar can target VS Code, Sublime, Prism.js, Shiki, Rouge (some support), and others.
4. **Scope standardization:** Scope names (e.g., `source.js`, `keyword.control`) became a de facto standard, adopted even by Pygments-derived systems (Linguist, GitHub).

**However:** TextMate's line-scoped constraint remains a fundamental limitation. Tree-sitter's full AST approach is strictly more powerful but requires more setup (grammar authoring + query authoring).

---

## 7. GitHub's Adoption of Tree-Sitter

GitHub migrated code highlighting from Linguist (Pygments-based) to tree-sitter-highlight. Key observations:

- **Scope alignment:** Tree-sitter scope names deliberately match TextMate/Linguist conventions, ensuring theme compatibility.
- **Accuracy gains:** Multi-line constructs, nested syntax, language injection now handled correctly.
- **Centralized grammar repos:** GitHub maintains or sponsors grammar repos for popular languages (JavaScript, Ruby, Python, etc.).
- **Open query system:** `queries/highlights.scm` files are editable by the community, lowering the barrier to grammar improvement.

---

## 8. Recommendations for Quarto 2

**Tree-sitter is the right backbone for a new implementation** because:

1. **You already have tree-sitter readily available.** Reusing existing grammar infrastructure avoids vendor lock-in (TextMate) or API churn (Prism.js).
2. **Semantic accuracy:** Full AST parsing enables context-aware coloring (e.g., parameter vs. local variable distinction via locals query).
3. **Multi-line support:** No single-line regex limits; handles nested constructs, heredocs, embedded languages natively.
4. **GitHub precedent:** Tree-sitter is now the production choice for the largest code host, with active maintenance.
5. **Extensibility:** New languages and annotations (via query files) are author-friendly compared to regex lexer authoring.

**Caveats:**
- Language coverage is smaller than TextMate grammars (~100 vs. ~1000+). Consider a fallback or hybrid strategy if broad language support is critical.
- Build-time dependency (tree-sitter parser for each language). Slightly heavier than Prism.js for lightweight deployments, but negligible for modern build pipelines.

---

## Sources

- [Tree-sitter Syntax Highlighting Documentation](https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html)
- [Shiki Documentation](https://shiki.style)
- [Prism.js Documentation](https://prismjs.com)
- [Pygments Lexer Development](https://pygments.org/docs/lexerdevelopment/)
- [VS Code Syntax Highlight Guide](https://code.visualstudio.com/api/language-extensions/syntax-highlight-guide)
- [TextMate Language Grammars Manual](https://macromates.com/manual/en/language_grammars)
- [Oniguruma Regex Engine](https://en.wikipedia.org/wiki/Oniguruma)
- [GitHub Linguist Tree-Sitter Integration](https://github.com/github/linguist/issues/5746)
