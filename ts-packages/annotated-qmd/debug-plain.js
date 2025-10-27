import { readFileSync } from 'fs';
import { parseRustQmdDocument } from './dist/index.js';

const json = JSON.parse(readFileSync('examples/blog-post.json', 'utf-8'));
json.astContext.files[0].content = readFileSync('examples/blog-post.qmd', 'utf-8');

const doc = parseRustQmdDocument(json);

function findPlainBlocks(node, path = [], results = []) {
  if (node.kind === 'Plain') {
    results.push({ node, path: [...path] });
  }
  for (let i = 0; i < node.components.length; i++) {
    findPlainBlocks(node.components[i], [...path, `${node.kind}[${i}]`], results);
  }
  return results;
}

const plains = findPlainBlocks(doc);
console.log(`Found ${plains.length} Plain blocks\n`);

for (const { node, path } of plains) {
  if (node.start === 0 && node.end === 0) {
    console.log('❌ Plain with start=0, end=0:');
    console.log(`  Path: ${path.join(' > ')}`);
    console.log(`  Components: ${node.components.length}`);
    console.log(`  First component kinds: ${node.components.slice(0, 3).map(c => c.kind).join(', ')}`);

    // Try to extract text
    function getText(n) {
      if (n.kind === 'Str') return n.result;
      if (n.kind === 'Space') return ' ';
      return n.components.map(getText).join('');
    }

    const text = getText(node);
    console.log(`  Text: "${text.substring(0, 100)}"`);
  }
}
