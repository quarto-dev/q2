/**
 * The Quarto icon as an MCP `Icon` (BP-9, SEP-973), inlined as a data
 * URI so the bundled single-file build carries it (no fs read).
 *
 * Source: `hub-client/public/quarto-icon.svg`, minified by hand (the
 * Adobe Illustrator preamble, the `<style>` block, and the
 * presentation attributes are folded into plain `fill` attributes).
 * Four rounded quadrants in Quarto blue (#74AADB).
 *
 * Where it appears: the server `Implementation` record, the prompts,
 * and the resource template. It deliberately does NOT ride on every
 * tool definition — 24 tools × a ~600-char data URI is ~15 KB on every
 * `tools/list`, which lands in the agent's context each session for
 * pure chrome; host UIs fall back to the server icon for tools without
 * one.
 */

import type { Icon } from '@modelcontextprotocol/server';

const QUARTO_ICON_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120">' +
  '<g fill="#74AADB" fill-rule="evenodd" clip-rule="evenodd">' +
  '<path d="M55.8,56.8V1C26.1,3.1,2.6,27.1,1,56.8H55.8z"/>' +
  '<path d="M63.2,56.8H119C117.4,26.8,93.2,2.6,63.2,1V56.8z"/>' +
  '<path d="M55.8,64.2H1.1c2.1,29.2,25.4,52.6,54.6,54.6V64.2z"/>' +
  '<path d="M63.2,64.2V119c29.7-1.6,53.7-25.1,55.7-54.7H63.2z"/>' +
  '</g></svg>';

export const QUARTO_ICON: Icon = {
  src: `data:image/svg+xml;base64,${Buffer.from(QUARTO_ICON_SVG, 'utf8').toString('base64')}`,
  mimeType: 'image/svg+xml',
  sizes: ['any'],
};
