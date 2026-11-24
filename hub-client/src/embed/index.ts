/**
 * The surface a host building its own editor imports.
 *
 * PROTOTYPE. Today a host deep-imports hub-client's whole `Editor` and
 * swaps pieces of it at build time. With an `exports` entry pointing here,
 * a host instead wraps its own layout in `ProjectProvider` and places the
 * parts it wants; this file is the whole contract.
 */

import '../theme.css';
import '../index.css';
import '../ui.css';

// The project, and the parts bound to it.
export { ProjectProvider, useProject, type ProjectContextValue, type ProjectProviderProps } from './ProjectProvider';
export {
  ProjectDocument,
  ProjectFileToolbar,
  ProjectFileTree,
  ProjectOutline,
  ProjectPreview,
  ProjectSyncStatus,
  type CommentsMode,
  type ProjectPreviewProps,
} from './parts';
export type { RenderComment } from '@quarto/preview-renderer/types/diagnostic';
export { useFileActions, type FileActions } from './useFileActions';
export {
  useProjectConnection,
  type HostIdentity,
  type ProjectConnection,
  type ProjectConnectionOptions,
} from './useProjectConnection';

// The components under the parts, for a host that wires them itself.
export { default as DocumentEditor } from '../components/DocumentEditor';
export { default as FileTree } from '../components/FileTree';
export { default as OutlinePanel } from '../components/OutlinePanel';
export { default as PreviewRouter } from '../components/render/PreviewRouter';
export { default as SyncStatusBadge } from '../components/SyncStatusBadge';

// Layout pieces with no tie to the project.
export { default as SplitFrame, type SplitFrameHandle, type SplitFrameProps } from '../components/SplitFrame';
export { default as ViewToggleControl } from '../components/ViewToggleControl';

// Chrome a host can reuse for its own dialogs and theming.
export { usePreference } from '../hooks/usePreference';
export { default as ModalDialog } from '../components/ModalDialog';
export { ThemeProvider } from '../components/ThemeContext';
