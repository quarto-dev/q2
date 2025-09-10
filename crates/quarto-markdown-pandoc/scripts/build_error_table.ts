#!/usr/bin/env deno run --allow-read --allow-write --allow-env --allow-run

import * as fs from "node:fs";
import { basename } from "node:path";
import { assert } from "jsr:/@std/testing@0.224.0/asserts";

// deno-lint-ignore no-explicit-any
const result: any = [];

const leftJoin = <T>(lst1: T[], lst2: T[], key: (item: T) => string) => {
  const map = new Map<string, T>();
  for (const item of lst2) {
    map.set(key(item), item);
  }
  const result = lst1.map((item) => [item, map.get(key(item))]).filter((
    [, v],
  ) => v !== undefined);
  return result as [T, T][];
};

for (const file of fs.globSync("resources/error-corpus/*.qmd")) {
  const base = basename(file, ".qmd");
  const errorInfo = JSON.parse(
    Deno.readTextFileSync(`resources/error-corpus/${base}.json`),
  );
  const parseResult = new Deno.Command("cargo", {
    args: ["run", "--", "--_internal-report-error-state", "-i", file],
  });
  const output = await parseResult.output();
  const outputStdout = new TextDecoder().decode(output.stdout);
  const parseResultJson = JSON.parse(outputStdout);
  const { errorStates, tokens } = parseResultJson;
  const matches = leftJoin(
    tokens,
    errorInfo.captures,
    (e: any) => `${e.row}:${e.column}:${e.size}`,
  );
  if (errorStates.length < 1) {
    throw new Error(`Expected at least one error state for ${file}`);
  }
  errorInfo.captures = errorInfo.captures.map((capture: any) => {
    const match = matches.find(([, b]) => b === capture);
    assert(match);
    return {...match[0], ...match[1]};
  });
  result.push({
    ...errorStates[0],
    errorInfo,
    name: `${base}`,
  });
}

console.log(JSON.stringify(result, null, 2));
