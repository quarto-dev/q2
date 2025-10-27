import { readFileSync } from 'fs';
import { parseRustQmdDocument } from './dist/index.js';

const json = JSON.parse(readFileSync('examples/simple.json', 'utf-8'));
json.astContext.files[0].content = readFileSync('examples/simple.qmd', 'utf-8');

const doc = parseRustQmdDocument(json);
const metadata = doc.components[0];

console.log('Metadata kind:', metadata.kind);
console.log('Metadata start/end:', metadata.start, metadata.end);
console.log('Metadata components count:', metadata.components.length);
console.log('\nFirst 10 components:');
for (let i = 0; i < Math.min(10, metadata.components.length); i++) {
  const comp = metadata.components[i];
  console.log(`  [${i}] kind=${comp.kind}, start=${comp.start}, end=${comp.end}`);
}

// Check for any blog-post with Plain end=0 issue
console.log('\n\nChecking blog-post for Plain with end=0:');
const json2 = JSON.parse(readFileSync('examples/blog-post.json', 'utf-8'));
json2.astContext.files[0].content = readFileSync('examples/blog-post.qmd', 'utf-8');

const doc2 = parseRustQmdDocument(json2);

function findPlainBlocks(node, results = []) {
  if (node.kind === 'Plain') {
    results.push(node);
  }
  for (const comp of node.components) {
    findPlainBlocks(comp, results);
  }
  return results;
}

const plains = findPlainBlocks(doc2);
console.log(`Found ${plains.length} Plain blocks`);
for (const plain of plains.slice(0, 5)) {
  console.log(`  Plain: start=${plain.start}, end=${plain.end}, components=${plain.components.length}`);
}
