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

console.log("Building pampa...");
await (new Deno.Command("cargo", {
  args: ["build"],
})).output();

// Create case-files directory for generated test files
const caseFilesDir = "resources/error-corpus/case-files";
try {
  await Deno.remove(caseFilesDir, { recursive: true });
} catch {
  // Directory doesn't exist, that's fine
}
await Deno.mkdir(caseFilesDir, { recursive: true });
console.log(`Using case files directory: ${caseFilesDir}`);

try {
  // Process both old format (.qmd files) and new format (.json files)
  const qmdFiles = Array.from(fs.globSync("resources/error-corpus/*.qmd")).toSorted((a, b) => a.localeCompare(b));
  const jsonFiles = Array.from(fs.globSync("resources/error-corpus/*.json"))
    .filter(f => !f.endsWith("_autogen-table.json"))
    .toSorted((a, b) => a.localeCompare(b));

  // Process old format .qmd files
  for (const file of qmdFiles) {
    const base = basename(file, ".qmd");
    const jsonPath = `resources/error-corpus/${base}.json`;

    // Skip numbered files if we have a consolidated Q-*.json version
    if (/^\d+$/.test(base)) {
      const errorInfo = JSON.parse(Deno.readTextFileSync(jsonPath));
      const consolidatedFile = `resources/error-corpus/${errorInfo.code}.json`;
      if (jsonFiles.includes(consolidatedFile)) {
        // Skip - will be processed from consolidated file
        continue;
      }
    }

    console.log(`Processing ${file}`);

    // Process old format .qmd file
    if (jsonFiles.some(jf => basename(jf, ".json") === base)) {
      const errorInfo = JSON.parse(Deno.readTextFileSync(jsonPath));
      const parseResult = new Deno.Command("../../target/debug/pampa", {
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
  }

  // Process new format .json files with cases
  for (const jsonFile of jsonFiles) {
    const base = basename(jsonFile, ".json");

    // Skip if there's a .qmd file (old format)
    if (qmdFiles.some(qf => basename(qf, ".qmd") === base)) {
      continue;
    }

    console.log(`Processing ${jsonFile} (new format)`);
    const errorSpec = JSON.parse(Deno.readTextFileSync(jsonFile));

    // Check if this is new format (has cases array)
    if (!errorSpec.cases || !Array.isArray(errorSpec.cases)) {
      console.log(`  Skipping ${jsonFile} - not new format (no cases array)`);
      continue;
    }

    const { code, title, message, notes, hints, cases } = errorSpec;

    // Process each case
    for (const testCase of cases) {
      const { name, content, captures, prefixes, suffixes, prefixesAndSuffixes } = testCase;
      console.log(`  Processing case: ${name}`);

      // Track (lr_state, sym) pairs for this case to detect duplicates
      const lrStateSyms = new Map<string, string>(); // key: "lrState:sym", value: variantName

      // Helper function to process a single variant (base or prefixed)
      const processVariant = async (
        variantName: string,
        variantContent: string,
        variantCaptures: any[]
      ) => {
        // Write content to case-files directory
        const caseFile = `${caseFilesDir}/${code}-${variantName}.qmd`;
        await Deno.writeTextFile(caseFile, variantContent);

        // Run parser with error state reporting
        const parseResult = new Deno.Command("../../target/debug/pampa", {
          args: ["--_internal-report-error-state", "-i", caseFile],
        });
        const output = await parseResult.output();
        
        const outputStdout = new TextDecoder().decode(output.stdout);
        let parseResultJson;
        try {
          parseResultJson = JSON.parse(outputStdout);
        } catch (e) {
          console.log(`Case file: ${caseFile} didn't produce errors`);
          throw e;
        } 
        const { errorStates, tokens } = parseResultJson;

        if (errorStates.length < 1) {
          throw new Error(`Expected at least one error state for ${code}/${variantName}`);
        }

        // Match and augment captures
        const looseMatching = variantCaptures.some((e: any) => e.size === undefined);
        const matches = looseMatching ?
          leftJoin(
            tokens,
            variantCaptures,
            (tok: any, cap: any) => tok.row === cap.row && tok.column === cap.column &&
              (cap.size !== undefined ? tok.size === cap.size : true)
          )
          : leftKeyJoin(
            tokens,
            variantCaptures,
            (e: any) => e.size ? `${e.row}:${e.column}:${e.size}` : `${e.row}:${e.column}`
          );

        const augmentedCaptures = variantCaptures.map((capture: any) => {
          const match = matches.find(([, b]) => b === capture);
          assert(match, `Could not find match for capture in ${code}/${variantName}`);
          return { ...match[0], ...match[1] };
        });

        // Check for duplicate (lr_state, sym) pairs within this case's captures
        for (const cap of augmentedCaptures) {
          const key = `${cap.lrState}:${cap.sym}`;
          const existing = lrStateSyms.get(key);
          if (existing) {
            console.warn(
              `⚠️  Warning: Duplicate (lr_state, sym) pair in ${code}/${name}:\n` +
              `    (${cap.lrState}, "${cap.sym}") appears in both:\n` +
              `      - ${existing}\n` +
              `      - ${variantName}\n` +
              `    This prefix does not currently generate a distinct parser state.\n` +
              `    Future grammar changes may make this prefix useful.`
            );
          }
          lrStateSyms.set(key, variantName);
        }

        // Create autogen table entry
        result.push({
          ...errorStates[0],
          errorInfo: {
            code,
            title,
            message,
            captures: augmentedCaptures,
            notes,
            hints: hints || [],
          },
          name: `${code}/${variantName}`,
        });
      };

      // Always process base case
      await processVariant(name, content, captures);

      // Process variants based on what's specified
      if (prefixesAndSuffixes && Array.isArray(prefixesAndSuffixes) && prefixesAndSuffixes.length > 0) {
        // prefixesAndSuffixes: loop once over pairs
        for (let i = 0; i < prefixesAndSuffixes.length; i++) {
          const [prefix, suffix] = prefixesAndSuffixes[i];
          const variantName = `${name}-${i + 1}`;
          const variantContent = prefix + content + suffix;
          const variantCaptures = captures.map((cap: any) => ({
            ...cap,
            column: cap.column + prefix.length,
          }));

          console.log(`    Processing prefix+suffix variant: ${variantName} (prefix: "${prefix}", suffix: "${suffix}")`);
          await processVariant(variantName, variantContent, variantCaptures);
        }
      } else if (prefixes && suffixes &&
                 Array.isArray(prefixes) && prefixes.length > 0 &&
                 Array.isArray(suffixes) && suffixes.length > 0) {
        // prefixes + suffixes: nested loop over all combinations
        let variantIndex = 0;
        for (const prefix of prefixes) {
          for (const suffix of suffixes) {
            variantIndex++;
            const variantName = `${name}-${variantIndex}`;
            const variantContent = prefix + content + suffix;
            const variantCaptures = captures.map((cap: any) => ({
              ...cap,
              column: cap.column + prefix.length,
            }));

            console.log(`    Processing prefix×suffix variant: ${variantName} (prefix: "${prefix}", suffix: "${suffix}")`);
            await processVariant(variantName, variantContent, variantCaptures);
          }
        }
      } else if (prefixes && Array.isArray(prefixes) && prefixes.length > 0) {
        // prefixes only: simple loop
        for (let i = 0; i < prefixes.length; i++) {
          const prefix = prefixes[i];
          const variantName = `${name}-${i + 1}`;
          const variantContent = prefix + content;
          const variantCaptures = captures.map((cap: any) => ({
            ...cap,
            column: cap.column + prefix.length,
          }));

          console.log(`    Processing prefix variant: ${variantName} (prefix: "${prefix}")`);
          await processVariant(variantName, variantContent, variantCaptures);
        }
      }
    }
  }
} finally {
  // Nothing to clean up - case files are kept for tests
}

Deno.writeTextFileSync("resources/error-corpus/_autogen-table.json", JSON.stringify(result, null, 2) + "\n");

const now = new Date();
// Touch the source file so that cargo build rebuilds it.
Deno.utimeSync("src/readers/qmd_error_message_table.rs", now, now);

console.log("Rebuilding pampa with new table...");
await (new Deno.Command("cargo", {
  args: ["build"],
})).output();
