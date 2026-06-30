// crates/quarto-core/tests/fixtures/extensions/echo-legacy/src/echo-legacy.ts
var _quarto;
var echoLegacyEngine = {
  name: "echolegacy",
  defaultExt: ".qmd",
  defaultYaml: (_kernel) => [],
  defaultContent: (_kernel) => [],
  validExtensions: () => [],
  canFreeze: false,
  generatesFigures: false,
  init(quarto) {
    _quarto = quarto;
  },
  claimsLanguage(language, _firstClass) {
    return language === "echolegacy";
  },
  claimsFile(_file, _ext) {
    return false;
  },
  launch(_context) {
    return {
      name: "echolegacy",
      canFreeze: false,
      async markdownForFile(file) {
        const text = Deno.readTextFileSync(file);
        const wrapped = "```{echolegacy}\n" + text + "\n```\n";
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
        const executed = input.replace(/```\{echolegacy\}[\s\S]*?```/g, "**ECHOLEGACY_EXECUTED**");
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
var echo_legacy_default = echoLegacyEngine;
export {
  echo_legacy_default as default
};
