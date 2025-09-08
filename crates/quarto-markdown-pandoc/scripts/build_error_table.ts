#!/usr/bin/env deno run --allow-read --allow-write --allow-env --allow-run

import * as fs from "node:fs";
import { basename } from "node:path";

// deno-lint-ignore no-explicit-any
const result: any = [];

for (const file of fs.globSync("resources/error-corpus/*.qmd")) {
  const base = basename(file, ".qmd");
  const errorMsg = Deno.readTextFileSync(`resources/error-corpus/${base}.txt`);
  const parseResult = new Deno.Command("cargo", {
    args: ["run", "--", "--_internal-report-error-state", "-i", file],
  });
  const output = await parseResult.output();
  const outputStdout = new TextDecoder().decode(output.stdout);
  const reportedError = JSON.parse(outputStdout);
  result.push({
    ...reportedError,
    errorMsg: errorMsg,
  });
}

console.log(JSON.stringify(result, null, 2));
