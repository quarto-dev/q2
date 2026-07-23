/**
 * Bundle Monaco instead of CDN-loading it (bd-yvz2xqrm).
 *
 * Without loader.config({ monaco }), @monaco-editor/loader fetches Monaco
 * from cdn.jsdelivr.net at runtime — unacceptable for an offline-first app:
 * a slow/blocked CDN left the source editor stuck at "Loading..." while the
 * preview (fed by the local websocket) worked. Bundling also lets the PWA
 * precache the editor with the rest of the app.
 *
 * Imported for its side effects from Editor.tsx (the only MonacoEditor
 * consumer). Gated by e2e/monaco-bundled.spec.ts, which blocks the CDN.
 */

import * as monaco from 'monaco-editor';
import { loader } from '@monaco-editor/react';
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker';
import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker';
import htmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker';
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker';

self.MonacoEnvironment = {
  getWorker(_workerId: string, label: string): Worker {
    switch (label) {
      case 'json':
        return new jsonWorker();
      case 'css':
      case 'scss':
      case 'less':
        return new cssWorker();
      case 'html':
      case 'handlebars':
      case 'razor':
        return new htmlWorker();
      case 'typescript':
      case 'javascript':
        return new tsWorker();
      default:
        return new editorWorker();
    }
  },
};

loader.config({ monaco });
