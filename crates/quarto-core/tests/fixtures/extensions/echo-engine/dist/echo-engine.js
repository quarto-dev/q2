// crates/quarto-core/tests/fixtures/extensions/echo-engine/src/echo-engine.ts
var _quarto;
var echoEngine = {
  name: "echo",
  defaultExt: ".echo",
  defaultYaml: (_kernel) => [],
  defaultContent: (_kernel) => [],
  validExtensions: () => [
    ".echo"
  ],
  canFreeze: false,
  generatesFigures: false,
  init(quarto) {
    _quarto = quarto;
  },
  claimsLanguage(language, _firstClass) {
    return language === "echo";
  },
  claimsFile(_file, ext) {
    return ext === ".echo";
  },
  launch(_context) {
    return {
      name: "echo",
      canFreeze: false,
      async markdownForFile(file) {
        const text = Deno.readTextFileSync(file);
        const wrapped = "```{echo}\n" + text + "\n```\n\n```{python}\nprint('not run by echo')\n```\n";
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
        const executed = input.replace(/```\{echo\}[\s\S]*?```/g, "**ECHO_EXECUTED**");
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
var echo_engine_default = echoEngine;
export {
  echo_engine_default as default
};
