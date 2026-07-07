// crates/quarto-core/tests/fixtures/extensions/content-claim/src/content-claim.ts
var _quarto;
var contentClaimEngine = {
  name: "content-claim",
  defaultExt: ".syn",
  defaultYaml: (_kernel) => [],
  defaultContent: (_kernel) => [],
  validExtensions: () => [
    ".syn"
  ],
  canFreeze: false,
  generatesFigures: false,
  init(quarto) {
    _quarto = quarto;
  },
  claimsLanguage(_language, _firstClass) {
    return false;
  },
  claimsFile(file, ext) {
    if (ext !== ".syn") return false;
    try {
      const text = Deno.readTextFileSync(file);
      const firstLine = text.split(/\r?\n/, 1)[0];
      return firstLine === "# synth-claim";
    } catch {
      return false;
    }
  },
  launch(_context) {
    return {
      name: "content-claim",
      canFreeze: false,
      async markdownForFile(file) {
        const text = Deno.readTextFileSync(file);
        const basename = file.split(/[\\/]/).pop() ?? file;
        const wrapped = "# Content-claimed: " + basename + "\n\n```{content-claim}\n" + text + "\n```\n";
        return _quarto.mappedString.fromString(wrapped, file);
      },
      async target(file, _quiet, markdown) {
        const ms = markdown ?? await this.markdownForFile(file);
        return {
          source: file,
          input: file,
          markdown: ms,
          metadata: {},
          data: void 0
        };
      },
      async partitionedMarkdown(file, _format) {
        const ms = await this.markdownForFile(file);
        return {
          markdown: ms.value,
          yaml: void 0,
          headingText: void 0,
          headingAttr: void 0,
          containsRefs: false,
          srcMarkdownNoYaml: ms.value
        };
      },
      async execute(opts) {
        const input = opts.target.markdown.value;
        const executed = input.replace(/```\{content-claim\}[\s\S]*?```/g, "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n**CONTENT_CLAIM_EXECUTED**\n:::\n:::");
        return {
          markdown: executed,
          supporting: [],
          filters: []
        };
      },
      async dependencies(_opts) {
        return {
          includes: {}
        };
      },
      async postprocess(_opts) {
      }
    };
  }
};
var content_claim_default = contentClaimEngine;
export {
  content_claim_default as default
};
