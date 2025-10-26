import { readFileSync } from 'fs';
import { parseRustQmdDocument } from './dist/index.js';

const json = JSON.parse(readFileSync('examples/minimal-figure.json', 'utf-8'));
json.astContext.files[0].content = readFileSync('examples/minimal-figure.qmd', 'utf-8');

console.log('=== Source Info Pool ===');
console.log('Pool size:', json.astContext.sourceInfoPool.length);

// Check specific source IDs
const sourceIds = [17, 25];
for (const id of sourceIds) {
  const entry = json.astContext.sourceInfoPool[id];
  console.log(`\nSource ID ${id}:`, entry);
}

const doc = parseRustQmdDocument(json);

function findFigures(node, path = [], results = []) {
  if (node.kind === 'Figure') {
    results.push({ node, path: [...path] });
  }
  for (let i = 0; i < node.components.length; i++) {
    findFigures(node.components[i], [...path, `${node.kind}[${i}]`], results);
  }
  return results;
}

const figures = findFigures(doc);
console.log(`\n=== Found ${figures.length} Figures ===\n`);

for (const { node, path } of figures) {
  console.log(`Figure at: ${path.join(' > ')}`);
  console.log(`  Figure source: start=${node.start}, end=${node.end}`);
  console.log(`  Components: ${node.components.length}`);

  for (let i = 0; i < node.components.length; i++) {
    const comp = node.components[i];
    console.log(`  [${i}] ${comp.kind}: start=${comp.start}, end=${comp.end}`);

    if (comp.kind === 'Plain') {
      console.log(`      ⚠️  Plain block: start=${comp.start}, end=${comp.end}, components=${comp.components.length}`);
    }
  }
  console.log();
}
