#!/usr/bin/env deno run --allow-read --allow-write --allow-env --allow-run

import * as fs from "node:fs";
import { basename } from "node:path";
import { assert } from "jsr:/@std/testing@0.224.0/asserts";

// deno-lint-ignore no-explicit-any
const result: any = [];

const leftKeyJoin = <T>(lst1: T[], lst2: T[], key: (item: T) => string) => {
  const map = new Map<string, T>();
  for (const item of lst2) {
    map.set(key(item), item);
  }
  const result = lst1.map((item) => [item, map.get(key(item))]).filter((
    [, v],
  ) => v !== undefined);
  return result as [T, T][];
};

const leftJoin = <T1, T2>(lst1: T1[], lst2: T2[], match: (i1: T1, i2: T2) => boolean) => {
  const result: [T1, T2][] = [];
  for (const i1 of lst1) {
    for (const i2 of lst2) {
      if (match(i1, i2)) {
        result.push([i1, i2]);
      }
    }
  }
  return result;
};

console.log("Building quarto-markdown-pandoc...");
await (new Deno.Command("cargo", {
  args: ["build"],
})).output();

const files = Array.from(fs.globSync("resources/error-corpus/*.qmd")).toSorted((a, b) => a.localeCompare(b));
for (const file of files) {
  console.log(`Processing ${file}`);
  const base = basename(file, ".qmd");
  const errorInfo = JSON.parse(
    Deno.readTextFileSync(`resources/error-corpus/${base}.json`),
  );
  const parseResult = new Deno.Command("../../target/debug/quarto-markdown-pandoc", {
    args: ["--_internal-report-error-state", "-i", file],
  });
  const output = await parseResult.output();
  const outputStdout = new TextDecoder().decode(output.stdout);
  const parseResultJson = JSON.parse(outputStdout);
  const { errorStates, tokens } = parseResultJson;
  
  const looseMatching = (errorInfo.captures.some((e: any) => e.size === undefined));

  const matches = looseMatching ? 
    leftJoin(
      tokens,
      errorInfo.captures,
      (tok: any, cap: any) => tok.row === cap.row && tok.column === cap.column && (cap.size !== undefined ? tok.size === cap.size : true)
    )
  : leftKeyJoin(
    tokens,
    errorInfo.captures,
    (e: any) => e.size ? `${e.row}:${e.column}:${e.size}` : `${e.row}:${e.column}`,
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

Deno.writeTextFileSync("resources/error-corpus/_autogen-table.json", JSON.stringify(result, null, 2) + "\n");

const now = new Date();
// Touch the source file so that cargo build rebuilds it.
Deno.utimeSync("src/readers/qmd_error_message_table.rs", now, now);

console.log("Rebuilding quarto-markdown-pandoc with new table...");
await (new Deno.Command("cargo", {
  args: ["build"],
})).output();
