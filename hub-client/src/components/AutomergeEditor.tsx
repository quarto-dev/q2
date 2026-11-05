/**
 * Monaco bound to a text field of an Automerge document.
 *
 * Local edits become `A.splice` calls on the handle; remote changes are
 * diffed against the editor's model and applied with `executeEdits`, so
 * the cursor survives. The `applyingRemote` flag stops those edits from
 * echoing back into the document.
 */

import { useEffect, useRef } from 'react';
import '../monacoSetup';
import MonacoEditor from '@monaco-editor/react';
import type * as Monaco from 'monaco-editor';
import { next as A } from '@automerge/automerge';
import type { DocHandle } from '@automerge/automerge-repo';
import { diffToMonacoEdits } from '../utils/diffToMonacoEdits';
import { QMD_LANGUAGE_ID, registerQmdLanguage } from './quartoTheme';

interface Props<T> {
  handle: DocHandle<T>;
  /** Path of the text field inside the document, e.g. `['text']`. */
  path: A.Prop[];
  /** Monaco language id; defaults to the hub's `qmd` grammar. */
  language?: string;
  /** Monaco theme; defaults to the hub's `quarto-light` (or pass `quarto-dark`). */
  theme?: string;
}

function textAt(doc: unknown, path: A.Prop[]): string {
  const value = path.reduce<unknown>((o, key) => (o as Record<A.Prop, unknown> | undefined)?.[key], doc);
  return typeof value === 'string' ? value : '';
}

export default function AutomergeEditor<T>({ handle, path, language = QMD_LANGUAGE_ID, theme = 'quarto-light' }: Props<T>) {
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const applyingRemote = useRef(false);

  const syncFromDoc = () => {
    const model = editorRef.current?.getModel();
    if (!model) return;
    const target = textAt(handle.doc(), path);
    const edits = diffToMonacoEdits(model.getValue(), target);
    if (edits.length === 0) return;
    applyingRemote.current = true;
    editorRef.current?.executeEdits('automerge', edits);
    applyingRemote.current = false;
  };

  useEffect(() => {
    handle.on('change', syncFromDoc);
    return () => { handle.off('change', syncFromDoc); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [handle]);

  const onChange = (_value: string | undefined, event: Monaco.editor.IModelContentChangedEvent) => {
    if (applyingRemote.current) return;
    handle.change((doc: T) => {
      for (const change of event.changes) {
        A.splice(doc as A.Doc<unknown>, path, change.rangeOffset, change.rangeLength, change.text);
      }
    });
  };

  return (
    <MonacoEditor
      key={handle.documentId}
      loading=""
      height="100%"
      language={language}
      theme={theme}
      defaultValue={textAt(handle.doc(), path)}
      // Registers the qmd grammar and quarto themes the hub editor uses.
      beforeMount={registerQmdLanguage}
      onMount={(editor) => { editorRef.current = editor; syncFromDoc(); }}
      onChange={onChange}
      options={{ minimap: { enabled: false }, fontSize: 14, wordWrap: 'on', scrollBeyondLastLine: false }}
    />
  );
}
