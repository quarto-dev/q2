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
  launch(context) {
    const capturedContext = context;
    const contextMarker = () => {
      const echoed = {
        dir: capturedContext.dir,
        isSingleFile: capturedContext.isSingleFile,
        config: capturedContext.config ?? null,
        outputDir: capturedContext.getOutputDirectory ? capturedContext.getOutputDirectory() : null
      };
      return "\n\n```\nCONTEXT_JSON_START" + JSON.stringify(echoed) + "CONTEXT_JSON_END\n```\n";
    };
    const formatMarker = (opts) => {
      const echoed = {
        execute: opts.format.execute,
        customKey: opts.format.metadata["echo-custom-key"]
      };
      return "\n\n```\nFORMAT_JSON_START" + JSON.stringify(echoed) + "FORMAT_JSON_END\n```\n";
    };
    return {
      name: "echo",
      canFreeze: false,
      async markdownForFile(file) {
        const text = Deno.readTextFileSync(file);
        const basename = file.split(/[\\/]/).pop() ?? file;
        const wrapped = "# Echoed: " + basename + "\n\n```{echo}\n" + text + "\n```\n\n```{python}\nprint('not run by echo')\n```\n";
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
        if (input.includes("QUARTO_ECHO_CRASH")) {
          console.error("ECHO_CRASH_MARKER: intentional crash for T13 crash-path e2e");
          Deno.exit(1);
        }
        const executed = input.replace(/```\{echo\}[\s\S]*?```/g, "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n**ECHO_EXECUTED**\n:::\n:::");
        return {
          markdown: executed + contextMarker() + formatMarker(opts),
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
