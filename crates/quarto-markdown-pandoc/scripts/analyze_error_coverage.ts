#!/usr/bin/env -S deno run --allow-read

/**
 * Analyzes the error corpus table to report coverage statistics per error code.
 * Shows how many distinct parser states are covered by each error code.
 */

interface ErrorTableEntry {
  state: number;
  sym: string;
  errorInfo: {
    code?: string;
    title: string;
    message: string;
  };
}

async function analyzeErrorCoverage(detailCode?: string) {
  const tablePath = "resources/error-corpus/_autogen-table.json";

  try {
    const tableContent = await Deno.readTextFile(tablePath);
    const entries: ErrorTableEntry[] = JSON.parse(tableContent);

    // Group by error code
    const codeStats = new Map<string, Set<number>>();
    const codeSyms = new Map<string, Set<string>>();
    const codeTitles = new Map<string, string>();

    for (const entry of entries) {
      const code = entry.errorInfo.code || "NO_CODE";

      if (!codeStats.has(code)) {
        codeStats.set(code, new Set());
        codeSyms.set(code, new Set());
        codeTitles.set(code, entry.errorInfo.title);
      }

      codeStats.get(code)!.add(entry.state);
      codeSyms.get(code)!.add(entry.sym);
    }

    // Sort codes and display
    const codes = Array.from(codeStats.keys()).sort();

    // If detail code requested, show only that
    if (detailCode) {
      if (!codeStats.has(detailCode)) {
        console.error(`Error code ${detailCode} not found in table`);
        Deno.exit(1);
      }

      const states = codeStats.get(detailCode)!;
      const syms = codeSyms.get(detailCode)!;
      const title = codeTitles.get(detailCode)!;
      const codeEntries = entries.filter(e => (e.errorInfo.code || "NO_CODE") === detailCode);

      console.log(`Error Code Details: ${detailCode}`);
      console.log("=".repeat(80));
      console.log(`Title: ${title}`);
      console.log(`States covered: ${states.size}`);
      console.log(`Symbols used: ${Array.from(syms).join(", ")}`);
      console.log(`Total state×symbol pairs: ${codeEntries.length}`);
      console.log();
      console.log("Parser States:");
      const sortedStates = Array.from(states).sort((a, b) => a - b);
      for (const state of sortedStates) {
        const stateEntries = codeEntries.filter(e => e.state === state);
        // Group by symbol and count
        const symCounts = new Map<string, number>();
        for (const entry of stateEntries) {
          symCounts.set(entry.sym, (symCounts.get(entry.sym) || 0) + 1);
        }
        const symStr = Array.from(symCounts.entries())
          .map(([sym, count]) => count > 1 ? `${sym} (×${count})` : sym)
          .join(", ");
        console.log(`  State ${state.toString().padStart(5)}: ${symStr}`);
      }
      return;
    }

    console.log("Error Corpus Coverage Analysis");
    console.log("=".repeat(80));
    console.log();

    let totalStates = 0;
    let totalSymbols = 0;

    for (const code of codes) {
      const states = codeStats.get(code)!;
      const syms = codeSyms.get(code)!;
      const title = codeTitles.get(code)!;

      totalStates += states.size;
      totalSymbols += syms.size;

      console.log(`${code.padEnd(10)} ${title}`);
      console.log(`${"".padEnd(10)} States: ${states.size.toString().padStart(4)}   Symbols: ${syms.size.toString().padStart(3)}   State×Symbol pairs: ${entries.filter(e => (e.errorInfo.code || "NO_CODE") === code).length.toString().padStart(4)}`);
      console.log();
    }

    console.log("=".repeat(80));
    console.log(`Total: ${codes.length} error codes`);
    console.log(`Total unique states covered: ${totalStates}`);
    console.log(`Total unique symbols used: ${totalSymbols}`);
    console.log(`Total state×symbol pairs: ${entries.length}`);

    // Find codes with most coverage
    console.log();
    console.log("Top 5 codes by state coverage:");
    const sortedByStates = codes
      .map(code => ({ code, states: codeStats.get(code)!.size, title: codeTitles.get(code)! }))
      .sort((a, b) => b.states - a.states)
      .slice(0, 5);

    for (const { code, states, title } of sortedByStates) {
      console.log(`  ${states.toString().padStart(4)} states  ${code.padEnd(10)} ${title}`);
    }

  } catch (error) {
    console.error("Error reading or parsing table:", error.message);
    Deno.exit(1);
  }
}

if (import.meta.main) {
  const detailCode = Deno.args[0];
  analyzeErrorCoverage(detailCode);
}
