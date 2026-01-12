// esbuild configuration for building JS bundles
// Run with: npm run build
// Or manually: node esbuild.config.mjs
//
// The generated bundles are committed to git and included via include_str!()
// in the Rust code. Rebuild only when updating JS dependencies or code.

import * as esbuild from 'esbuild';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const args = process.argv.slice(2);
const simpleOnly = args.includes('--simple-only');
const ejsOnly = args.includes('--ejs-only');

async function buildSimpleTemplate() {
    console.log('Building simple-template-bundle.js...');

    // Simple template doesn't need bundling, just copy with wrapper
    const src = fs.readFileSync(path.join(__dirname, 'src/simple-template.js'), 'utf-8');
    const bundle = `// Simple template bundle for interstitial testing
// DO NOT EDIT - generated from js/src/simple-template.js

(function() {
    "use strict";
${src.split('\n').map(line => '    ' + line).join('\n')}
})();
`;
    fs.writeFileSync(path.join(__dirname, 'dist/simple-template-bundle.js'), bundle);
    console.log('  -> dist/simple-template-bundle.js');
}

async function buildEjsBundle() {
    console.log('Building ejs-bundle.js...');

    // Create a temporary entry point that imports EJS and exposes it globally
    const entryContent = `
import ejs from 'ejs';
globalThis.ejs = ejs;
`;
    const entryPath = path.join(__dirname, 'src/.ejs-entry-temp.js');
    fs.writeFileSync(entryPath, entryContent);

    try {
        await esbuild.build({
            entryPoints: [entryPath],
            bundle: true,
            outfile: path.join(__dirname, 'dist/ejs-bundle.js'),
            format: 'iife',
            platform: 'browser',
            target: ['es2020'],
            minify: false, // Keep readable for debugging
            sourcemap: false,
        });
        console.log('  -> dist/ejs-bundle.js');
    } finally {
        // Clean up temp file
        fs.unlinkSync(entryPath);
    }
}

async function main() {
    try {
        if (!ejsOnly) {
            await buildSimpleTemplate();
        }
        if (!simpleOnly) {
            await buildEjsBundle();
        }
        console.log('Build complete!');
    } catch (error) {
        console.error('Build failed:', error);
        process.exit(1);
    }
}

main();
