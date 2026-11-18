// Runs inside the VS Code extension host (see run.mjs). Opens the sample
// document and checks that q2 lsp's semantic tokens reach VS Code with the
// legend the extension declares. No test framework: throwing fails the run.
const assert = require('node:assert/strict');
const path = require('node:path');
const vscode = require('vscode');

async function waitFor(fn, what, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await fn();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 250));
  }
}

exports.run = async function run() {
  const file = path.join(__dirname, '..', 'test-workspace', 'sample.qmd');
  const doc = await vscode.workspace.openTextDocument(file);
  assert.equal(doc.languageId, 'quarto2');
  await vscode.window.showTextDocument(doc);

  const legend = await waitFor(
    () => vscode.commands.executeCommand('vscode.provideDocumentSemanticTokensLegend', doc.uri),
    'a semantic tokens legend',
  );
  for (const t of ['qmdHeading', 'keyword', 'string']) {
    assert.ok(legend.tokenTypes.includes(t), `legend lacks ${t}: ${legend.tokenTypes}`);
  }

  const tokens = await waitFor(async () => {
    const t = await vscode.commands.executeCommand('vscode.provideDocumentSemanticTokens', doc.uri);
    return t && t.data.length > 0 ? t : undefined;
  }, 'semantic tokens');

  // Decode and find the heading on line 5 ("# A heading") and a keyword.
  const seen = new Set();
  let line = 0;
  for (let i = 0; i < tokens.data.length; i += 5) {
    line += tokens.data[i];
    seen.add(`${line}:${legend.tokenTypes[tokens.data[i + 3]]}`);
  }
  assert.ok(seen.has('5:qmdHeading'), `no heading token on line 5: ${[...seen].join(' ')}`);
  assert.ok([...seen].some((s) => s.endsWith(':keyword')), 'no keyword token');

  const symbols = await waitFor(
    () => vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', doc.uri),
    'document symbols',
  );
  assert.ok(symbols.some((s) => s.name === 'A heading'), JSON.stringify(symbols.map((s) => s.name)));
  console.log(`smoke: ${tokens.data.length / 5} tokens, ${symbols.length} symbols`);
};
