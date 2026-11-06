/**
 * The hub-client Monaco editor for one text document of the connected
 * project. Self-contained: given a file of the project the preview runtime
 * is connected to, it provides
 *
 * - bidirectional Automerge sync (`useAutomergeSync`),
 * - remote cursors and selections (`usePresence`),
 * - the `qmd` grammar and quarto-{light,dark} themes,
 * - the outline / folding / semantic-token providers (`useIntelligenceProviders`),
 * - Cmd/Ctrl+B bold toggling, link-on-paste and image-on-paste,
 * - diagnostics markers: the host's (`markers`, the render pipeline's in
 *   Editor.tsx) or, without them, the LSP's own analysis of the document,
 * - replay read-only mode, local branches,
 * - gutter compaction when narrow and the backward-selection typing fix.
 *
 * Everything project-level (file tree, preview, dialogs, drop uploads,
 * replay scrubbing) lives in `Editor.tsx`, which drives this component
 * through props, the `onMount` callback and the imperative handle. The
 * demo at `#/demo/monaco` renders it alone.
 */

import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
  type RefObject,
} from 'react';
// Side effects only: bundles Monaco + workers so no runtime CDN fetch occurs.
import '../monacoSetup';
import MonacoEditor from '@monaco-editor/react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { createBinaryFile, type EditorContentChange } from '@quarto/preview-runtime';
import { useAutomergeSync } from '../hooks/useAutomergeSync';
import { usePresence } from '../hooks/usePresence';
import { useIntelligenceProviders } from '../hooks/useIntelligenceProviders';
import { useIntelligence } from '../hooks/useIntelligence';
import { lspDiagnosticsToMarkers } from '../utils/diagnosticToMonaco';
import type { PresenceState } from '../services/presenceService';
import { processFileForUpload, FILE_SIZE_LIMITS } from '../services/resourceService';
import { classifyPastePayload, createPasteImageHandler } from './fileUpload';
import { registerQmdLanguage } from './quartoTheme';
import { linkFromPaste } from '../utils/pasteLink';
import { toggleWrap } from '../utils/markdownToggle';
import { getLanguageForFile } from '../utils/monacoLanguage';

export interface DocumentEditorHandle {
  /** The mounted Monaco instance; null before mount and while no file is open. */
  readonly editor: Monaco.editor.IStandaloneCodeEditor | null;
  /**
   * True while edits are being applied to Monaco that must not echo back
   * into Automerge. Set it around any `executeEdits`/`setValue` that
   * mirrors content from elsewhere (replay scrubbing does this).
   */
  readonly applyingRemoteRef: MutableRefObject<boolean>;
  /** Set the content state (preview/summary), e.g. on a file switch. */
  setContent: (content: string) => void;
  /** Apply an AST rewrite: write to Automerge, then sync Monaco (undo-safe). */
  rewriteContent: (content: string) => void;
}

export interface DocumentEditorProps {
  /** The text file bound to the editor, or null when nothing is open. */
  file: FileEntry | null;
  /** Path -> content of the project; a change triggers reconciliation. */
  fileContents: Map<string, string>;
  /** Propagate local edits to Automerge as positional splices. */
  onContentOperations: (path: string, changes: EditorContentChange[]) => void;
  /** Selects the quarto-light / quarto-dark theme. Default: light. */
  theme?: 'light' | 'dark';
  /**
   * Diagnostics markers (owner `quarto`) to underline. When omitted the
   * editor analyses the document with the LSP (quarto-lsp-core) on every
   * content change and shows those diagnostics instead.
   */
  markers?: Monaco.editor.IMarkerData[];
  /** Read-only mode (replay). Default: false. */
  readOnly?: boolean;
  /** Ref from useReplayMode — gates sync synchronously while replay is active. */
  replayActiveRef?: RefObject<boolean>;
  /** Whether replay is active (for effect dependencies). Default: false. */
  replayIsActive?: boolean;
  /** Active local branch id for the file, or null when on main. */
  activeBranchId?: string | null;
  /** Broadcast and render collaborative cursors. Default: true. */
  presenceEnabled?: boolean;
  /**
   * Status of the preview WASM module: a `loading → ready` transition
   * re-tokenises once. Default: `ready` (the providers initialise the
   * module themselves when there is no preview around).
   */
  wasmStatus?: 'loading' | 'ready' | 'error';
  /** Fires synchronously with every content update (local, remote, switch). */
  onContentChange?: (content: string) => void;
  /** Fires whenever the set of remote users on this file changes. */
  onPresenceChange?: (remoteUsers: PresenceState[]) => void;
  /**
   * The Monaco instance was created (again on every file/branch switch,
   * which remounts the editor). The intrinsic behaviours are already
   * attached; add project-level listeners here.
   */
  onMount?: (editor: Monaco.editor.IStandaloneCodeEditor, monaco: typeof Monaco) => void;
}

// Stable options object — defined at module level so @monaco-editor/react
// never sees a new reference and skips its internal updateOptions() effect.
const editorOptions = {
  minimap: { enabled: false },
  fontSize: 14,
  lineNumbers: 'on' as const,
  // Size the number column near the document's actual digit count
  // instead of Monaco's default 5-char reserve; the extra char over the
  // digits acts as left padding (numbers are right-aligned).
  lineNumbersMinChars: 4,
  wordWrap: 'on' as const,
  padding: { top: 16 },
  scrollBeyondLastLine: false,
  // Disable paste-as to prevent snippet expansion (e.g., URLs from browser
  // address bar being pasted with $0 appended). See quarto-dev/kyoto#3.
  pasteAs: { enabled: false },
  fixedOverflowWidgets: true,
  // Prefer showing hover below the line — prevents diagnostic popups near
  // the top of the editor from overlapping the navbar.
  hover: { above: false },
  quickSuggestions: false,
  suggestOnTriggerCharacters: false,
  wordBasedSuggestions: 'off' as const,
  acceptSuggestionOnEnter: 'off' as const,
  acceptSuggestionOnCommitCharacter: false,
  suggest: { showWords: false, showSnippets: false },
  inlineSuggest: { enabled: false },
  // Enable the semantic-tokens layer (the quarto-{light,dark} themes also set
  // `semanticHighlighting: true`; setting both is the reliable combination).
  'semanticHighlighting.enabled': true as const,
};

const NEVER_REPLAYING: RefObject<boolean> = { current: false };

// Configure Monaco before mount.
function handleBeforeMount(monaco: typeof Monaco): void {
  // Register the dedicated `qmd` language: Monarch base (Hybrid-A instant
  // paint + nextEmbedded routing of code/frontmatter) + quarto-{light,dark}
  // themes. The semantic-tokens provider is the authoritative colour source.
  registerQmdLanguage(monaco);

  // Disable TypeScript diagnostics to avoid noisy errors in TSX/TS files
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const languages = (monaco.languages as any);
  try {
    if (languages.typescript?.typescriptDefaults) {
      // Enable JSX support
      languages.typescript.typescriptDefaults.setCompilerOptions({
        jsx: languages.typescript.JsxEmit.React,
        jsxFactory: 'React.createElement',
        reactNamespace: 'React',
        allowNonTsExtensions: true,
        allowJs: true,
      });
      languages.typescript.typescriptDefaults.setDiagnosticsOptions({
        noSemanticValidation: true,
        noSyntaxValidation: true, // Disable all validation
      });
    }
    if (languages.typescript?.javascriptDefaults) {
      languages.typescript.javascriptDefaults.setCompilerOptions({
        jsx: languages.typescript.JsxEmit.React,
        jsxFactory: 'React.createElement',
        reactNamespace: 'React',
        allowNonTsExtensions: true,
      });
      languages.typescript.javascriptDefaults.setDiagnosticsOptions({
        noSemanticValidation: true,
        noSyntaxValidation: true, // Disable all validation
      });
    }
  } catch (e) {
    // Ignore if TypeScript language service is not available
    console.warn('Could not configure TypeScript diagnostics:', e);
  }
}

/** Cmd/Ctrl+<key> toggles `marker` around the selection (or inserts an empty pair at the cursor). */
function registerToggleAction(
  editor: Monaco.editor.IStandaloneCodeEditor,
  monaco: typeof Monaco,
  id: string,
  label: string,
  key: number,
  marker: string,
): void {
  editor.addAction({
    id,
    label,
    keybindings: [monaco.KeyMod.CtrlCmd | key],
    run: (ed) => {
      const model = ed.getModel();
      const sel = ed.getSelection();
      if (!model || !sel) return;
      const selected = model.getValueInRange(sel);
      const m = marker.length;
      const before = model.getValueInRange({
        startLineNumber: sel.startLineNumber,
        startColumn: Math.max(1, sel.startColumn - m),
        endLineNumber: sel.startLineNumber,
        endColumn: sel.startColumn,
      });
      const after = model.getValueInRange({
        startLineNumber: sel.endLineNumber,
        startColumn: sel.endColumn,
        endLineNumber: sel.endLineNumber,
        endColumn: sel.endColumn + m,
      });
      const r = toggleWrap(selected, marker, before, after);
      const range = {
        startLineNumber: sel.startLineNumber,
        startColumn: sel.startColumn - r.extendBefore,
        endLineNumber: sel.endLineNumber,
        endColumn: sel.endColumn + r.extendAfter,
      };
      ed.executeEdits(id, [{ range, text: r.text, forceMoveMarkers: true }]);
      // Re-select the inner text (or place the cursor between the markers).
      const startCol = range.startColumn + r.selectStart;
      const singleLine = sel.startLineNumber === sel.endLineNumber;
      ed.setSelection({
        startLineNumber: sel.startLineNumber,
        startColumn: startCol,
        endLineNumber: singleLine ? sel.startLineNumber : sel.endLineNumber,
        endColumn: singleLine
          ? range.startColumn + r.selectEnd
          : sel.endColumn + r.extendAfter - (r.text.length - r.selectEnd),
      });
    },
  });
}

const DocumentEditor = forwardRef<DocumentEditorHandle, DocumentEditorProps>(function DocumentEditor(
  {
    file,
    fileContents,
    onContentOperations,
    theme = 'light',
    markers,
    readOnly = false,
    replayActiveRef = NEVER_REPLAYING,
    replayIsActive = false,
    activeBranchId = null,
    presenceEnabled = true,
    wasmStatus = 'ready',
    onContentChange,
    onPresenceChange,
    onMount,
  },
  ref,
) {
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<typeof Monaco | null>(null);
  const [editorReady, setEditorReady] = useState(false);
  // Bumped per Monaco instance so per-instance effects (markers) re-run
  // after a file/branch switch recreates the editor.
  const [mountGen, setMountGen] = useState(0);

  // Latest callbacks behind refs: the mount handler and DOM listeners are
  // attached once per Monaco instance and must not go stale.
  const onMountRef = useRef(onMount);
  const onPresenceChangeRef = useRef(onPresenceChange);
  useEffect(() => {
    onMountRef.current = onMount;
    onPresenceChangeRef.current = onPresenceChange;
  });

  // Current file path for the Monaco providers (they need stable callbacks).
  const filePathRef = useRef<string | null>(file?.path ?? null);
  useEffect(() => {
    filePathRef.current = file?.path ?? null;
  }, [file]);
  const getCurrentFilePath = useCallback(() => filePathRef.current, []);

  // Bidirectional Automerge ↔ Monaco sync
  const {
    content,
    setContent,
    applyingRemoteRef,
    handleEditorChange,
    handleContentRewrite,
    onEditorMount: onSyncEditorMount,
  } = useAutomergeSync({
    currentFile: file,
    fileContents,
    onContentOperations,
    replayActiveRef,
    replayIsActive,
    activeBranchId,
    onContentChange,
  });

  // Collaborative cursors — the caller disables these on a local branch
  // (the branch doc is invisible to peers, so cursors would be misleading).
  const { remoteUsers, onEditorMount: onPresenceEditorMount } = usePresence(
    file?.path ?? null,
    { enabled: presenceEnabled },
  );
  useEffect(() => {
    onPresenceChangeRef.current?.(remoteUsers);
  }, [remoteUsers]);
  useEffect(() => () => { onPresenceChangeRef.current?.([]); }, []);

  // Monaco intelligence providers (symbols, folding, semantic tokens).
  const registerIntelligenceProvidersOnMount = useIntelligenceProviders(
    getCurrentFilePath,
    wasmStatus,
    editorReady,
  );

  useImperativeHandle(
    ref,
    () => ({
      get editor() { return editorRef.current; },
      applyingRemoteRef,
      setContent,
      rewriteContent: handleContentRewrite,
    }),
    [applyingRemoteRef, setContent, handleContentRewrite],
  );

  // Read-only mode (replay).
  useEffect(() => {
    editorRef.current?.updateOptions({ readOnly, domReadOnly: readOnly });
  }, [readOnly]);

  // Own diagnostics from the LSP, used when the host supplies no markers.
  // The analysis reads the VFS, which the sync keeps current, so a content
  // change is the trigger (debounced in the hook).
  const ownDiagnostics = markers === undefined;
  const { diagnostics: lspDiagnostics, refresh: refreshDiagnostics } = useIntelligence({
    path: ownDiagnostics ? (file?.path ?? null) : null,
    enableSymbols: false,
    enableDiagnostics: true,
  });
  useEffect(() => {
    if (ownDiagnostics) refreshDiagnostics();
  }, [content, ownDiagnostics, refreshDiagnostics]);
  const lspMarkers = useMemo(() => lspDiagnosticsToMarkers(lspDiagnostics).markers, [lspDiagnostics]);

  // Diagnostics markers.
  const effectiveMarkers = markers ?? lspMarkers;
  useEffect(() => {
    const model = editorRef.current?.getModel();
    if (!model || !monacoRef.current) return;
    monacoRef.current.editor.setModelMarkers(model, 'quarto', effectiveMarkers);
  }, [effectiveMarkers, mountGen]);

  // Clipboard image paste (bd-706b0ixu, see
  // claude-notes/plans/2026-08-27-paste-image-clipboard.md): silent
  // ingest — content-hash-named file next to the current document, then
  // a markdown reference at the cursor. Every dep reads through a ref or
  // a stable callback, so the handler is identity-stable and the DOM
  // listener can be attached once at editor mount.
  const pasteImageIngestRef = useRef(
    createPasteImageHandler({
      getCurrentFilePath,
      getEditor: () => {
        const editor = editorRef.current;
        if (!editor) return null;
        return {
          getSelection: () => editor.getSelection(),
          getTextInRange: (range) => editor.getModel()?.getValueInRange(range) ?? '',
          replaceRange: (range, text) => {
            // Monaco's onChange fires synchronously from executeEdits,
            // updating both React state and CRDT via the splice path.
            editor.executeEdits('image-paste', [{ range, text, forceMoveMarkers: true }]);
          },
        };
      },
      processFile: processFileForUpload,
      createBinaryFile,
      maxFileSize: FILE_SIZE_LIMITS.MAX_FILE_SIZE,
      onError: (message) => console.error(message),
    }),
  );

  const handlePaste = useCallback((e: ClipboardEvent) => {
    const clipboard = e.clipboardData;
    if (!clipboard) return;
    const files = Array.from(clipboard.files);

    // Pasting a URL over selected text wraps the selection as a link.
    const editor = editorRef.current;
    const selection = editor?.getSelection();
    if (editor && selection && files.length === 0) {
      const selected = editor.getModel()?.getValueInRange(selection) ?? '';
      const link = linkFromPaste(selected, clipboard.getData('text/plain'));
      if (link) {
        e.preventDefault();
        e.stopPropagation();
        editor.executeEdits('paste-link', [{ range: selection, text: link, forceMoveMarkers: true }]);
        return;
      }
    }

    if (
      classifyPastePayload({
        files: files.map((f) => ({ name: f.name, type: f.type, size: f.size })),
        text: clipboard.getData('text/plain'),
      }) !== 'take-over'
    ) {
      return; // text and mixed payloads: Monaco's paste handling runs
    }
    // Taking over: stop Monaco's own handler (it would insert the
    // filename rider as stray text) before the async ingest starts.
    e.preventDefault();
    e.stopPropagation();
    void pasteImageIngestRef.current(files);
  }, []);

  // Detach the paste listener on unmount only; a file switch disposes the
  // whole Monaco DOM node (and its listeners) with the old instance.
  useEffect(() => {
    return () => {
      editorRef.current?.getDomNode()?.removeEventListener('paste', handlePaste, true);
    };
  }, [handlePaste]);

  const handleEditorMount = (editor: Monaco.editor.IStandaloneCodeEditor, monaco: typeof Monaco) => {
    editorRef.current = editor;
    monacoRef.current = monaco;

    registerToggleAction(editor, monaco, 'qmd.toggleBold', 'Toggle bold', monaco.KeyCode.KeyB, '**');

    onSyncEditorMount(editor);
    onPresenceEditorMount(editor);

    // Compact the gutter when the editor gets narrow (split dragged
    // small): drop the line numbers and folding arrows, keeping a
    // sliver of decoration lane as padding.
    let gutterCompact: boolean | null = null;
    const applyNarrowLayout = (width: number) => {
      const compact = width < 400;
      if (compact === gutterCompact) return;
      gutterCompact = compact;
      editor.updateOptions(
        compact
          ? { lineNumbers: 'off', folding: false, lineDecorationsWidth: 4 }
          : { lineNumbers: 'on', folding: true, lineDecorationsWidth: 10 },
      );
    };
    applyNarrowLayout(editor.getLayoutInfo().width);
    editor.onDidLayoutChange((info) => applyNarrowLayout(info.width));

    // Register intelligence providers (symbols, folding, semantic tokens).
    // The getter uses a ref so it always returns the current file path.
    registerIntelligenceProvidersOnMount(monaco);

    // Workaround: normalize backward (RTL) selections to forward (LTR) on
    // any printable keyDown. On some platforms the browser's input pipeline
    // silently drops the first character typed into a backward selection
    // because the hidden textarea's selection state confuses the OS input
    // method. Flipping to LTR keeps the same highlighted range but places
    // the cursor at the end, which the input method handles reliably.
    editor.onKeyDown((e) => {
      const sel = editor.getSelection();
      if (!sel || sel.isEmpty() || sel.getDirection() === 0) return; // LTR or no selection
      // Only intervene for printable characters (single char from e.browserEvent.key)
      const key = e.browserEvent.key;
      if (!key || key.length !== 1) return;
      // Flip to LTR (same range, cursor moves to end)
      editor.setSelection({
        selectionStartLineNumber: sel.startLineNumber,
        selectionStartColumn: sel.startColumn,
        positionLineNumber: sel.endLineNumber,
        positionColumn: sel.endColumn,
      });
    });

    // Capture phase: the paste event targets Monaco's hidden textarea,
    // and Monaco's own listener (which preventDefaults and re-implements
    // text paste) sits on that textarea — capture on the container runs
    // first, so image payloads can be intercepted (bd-706b0ixu).
    editor.getDomNode()?.addEventListener('paste', handlePaste, true);

    if (readOnly) editor.updateOptions({ readOnly: true, domReadOnly: true });

    setEditorReady(true);
    setMountGen((gen) => gen + 1);
    onMountRef.current?.(editor, monaco);
  };

  return (
    <MonacoEditor
      // Key forces remount when switching files or branches
      // (resets editor state cleanly)
      key={`${file?.path ?? ''}::${activeBranchId ?? 'main'}`}
      // Blank fallback: monaco is bundled, so the only wait is the
      // per-mount editor-instance creation (a frame or two) — the
      // default "Loading..." text just flashes on every open and
      // file switch.
      loading=""
      height="100%"
      language={getLanguageForFile(file?.path ?? '')}
      theme={theme === 'dark' ? 'quarto-dark' : 'quarto-light'}
      // Use defaultValue instead of value to make Monaco uncontrolled.
      // This prevents the wrapper from calling setValue() on re-renders,
      // which would reset cursor position. We manage content via executeEdits().
      defaultValue={content}
      beforeMount={handleBeforeMount}
      onChange={handleEditorChange}
      onMount={handleEditorMount}
      options={editorOptions}
    />
  );
});

export default DocumentEditor;
