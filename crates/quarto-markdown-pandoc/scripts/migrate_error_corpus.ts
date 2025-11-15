#!/usr/bin/env deno run --allow-read --allow-write --allow-env

import * as fs from "node:fs";
import { basename } from "node:path";

interface Capture {
  label: string;
  row: number;
  column: number;
  size?: number;
}

interface Note {
  message: string;
  label?: string;
  noteType: string;
  labelBegin?: string;
  labelEnd?: string;
  trimLeadingSpace?: boolean;
}

interface OldErrorInfo {
  code: string;
  title: string;
  message: string;
  captures: Capture[];
  notes: Note[];
}

interface Case {
  name: string;
  description: string;
  content: string;
  captures: Capture[];
}

interface NewErrorSpec {
  code: string;
  title: string;
  message: string;
  notes: Note[];
  cases: Case[];
}

// Group old .qmd/.json files by error code
const errorCodeMap = new Map<string, Array<{ base: string; content: string; errorInfo: OldErrorInfo }>>();

const qmdFiles = Array.from(fs.globSync("resources/error-corpus/*.qmd"))
  .filter(f => {
    const base = basename(f, ".qmd");
    // Only migrate numbered files (old format)
    return /^\d+$/.test(base);
  })
  .toSorted((a, b) => a.localeCompare(b));

console.log(`Found ${qmdFiles.length} old-format .qmd files to migrate`);

for (const qmdFile of qmdFiles) {
  const base = basename(qmdFile, ".qmd");
  const jsonFile = `resources/error-corpus/${base}.json`;

  try {
    const content = Deno.readTextFileSync(qmdFile);
    const errorInfo = JSON.parse(Deno.readTextFileSync(jsonFile)) as OldErrorInfo;

    if (!errorCodeMap.has(errorInfo.code)) {
      errorCodeMap.set(errorInfo.code, []);
    }

    errorCodeMap.get(errorInfo.code)!.push({ base, content, errorInfo });
  } catch (e) {
    console.error(`Error processing ${qmdFile}:`, e);
  }
}

// Generate case names based on content analysis
function generateCaseName(content: string, index: number): string {
  const trimmed = content.trim();

  // Try to identify context from content
  if (trimmed.match(/^\*\*.*\*\*$/)) return "in-strong-emphasis";
  if (trimmed.match(/^\*.*\*$/)) return "in-emphasis";
  if (trimmed.match(/^__.*__$/)) return "in-underscore-strong";
  if (trimmed.match(/^_.*_$/)) return "in-underscore-emphasis";
  if (trimmed.match(/^~~.*~~$/)) return "in-strikethrough";
  if (trimmed.match(/^\^.*\^$/)) return "in-superscript";
  if (trimmed.match(/^~.*~$/)) return "in-subscript";
  if (trimmed.match(/^\[.*\]\(.*\)$/)) return "in-link";
  if (trimmed.match(/^!\[.*\]\(.*\)$/)) return "in-image";
  if (trimmed.match(/^\[\+\+.*\]$/)) return "in-editorial-insert";
  if (trimmed.match(/^\[--.*\]$/)) return "in-editorial-delete";
  if (trimmed.match(/^\[>>.*\]$/)) return "in-editorial-comment";
  if (trimmed.match(/^\[==.*\]$/)) return "in-editorial-highlight";
  if (trimmed.match(/^\[!!.*\]$/)) return "in-editorial-highlight-alt";
  if (trimmed.match(/\^\[.*\]$/)) return "in-inline-footnote";
  if (trimmed.match(/^#+\s/)) return "in-heading";
  if (trimmed.match(/^".*"$/)) return "in-double-quote";
  if (trimmed.match(/^'.*'$/)) return "in-single-quote";

  // Fallback to simple names
  if (index === 0) return "simple";
  return `variant-${index + 1}`;
}

function generateDescription(content: string, name: string): string {
  if (name === "simple") return "Simple case";
  if (name.startsWith("in-")) {
    const context = name.replace("in-", "").replace(/-/g, " ");
    return `Inside ${context}`;
  }
  return `Test case ${name}`;
}

// Create consolidated .json files for each error code
for (const [code, cases] of errorCodeMap.entries()) {
  console.log(`\nMigrating ${code} (${cases.length} cases)`);

  // Use the first case's error info as the template
  const template = cases[0].errorInfo;

  const newSpec: NewErrorSpec = {
    code,
    title: template.title,
    message: template.message,
    notes: template.notes,
    cases: cases.map((c, index) => {
      const name = generateCaseName(c.content, index);
      const description = generateDescription(c.content, name);

      console.log(`  ${c.base} → ${name} (${description})`);

      return {
        name,
        description,
        content: c.content,
        captures: c.errorInfo.captures,
      };
    }),
  };

  const outputFile = `resources/error-corpus/${code}.json`;
  Deno.writeTextFileSync(outputFile, JSON.stringify(newSpec, null, 2) + "\n");
  console.log(`  Wrote ${outputFile}`);
}

console.log(`\n✓ Migration complete. Created ${errorCodeMap.size} consolidated .json files.`);
console.log(`\nNext steps:`);
console.log(`1. Review the generated ${Array.from(errorCodeMap.keys()).join(", ")} files`);
console.log(`2. Run ./scripts/build_error_table.ts to rebuild the error table`);
console.log(`3. Run cargo test to verify everything works`);
console.log(`4. Move old numbered files to resources/error-corpus/old/ if tests pass`);
