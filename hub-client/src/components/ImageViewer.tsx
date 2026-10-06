/**
 * ImageViewer — replaces the editor and preview panes when an image file
 * is selected in the project sidebar.
 *
 * Image bytes live in a binary Automerge document (not in the React
 * `fileContents` map, which holds text only), so the viewer reads them
 * from the sync client and displays them through a data URL. The `version`
 * prop is bumped by App whenever the binary doc syncs or changes, which
 * re-reads the bytes; until they arrive the viewer shows a loading state.
 */

import { useMemo, useState } from 'react';
import { getBinaryFileContent } from '@quarto/preview-runtime';
import { binaryToDataUrl } from '../services/resourceService';
import { formatByteSize } from './imageViewerFormat';
import './ImageViewer.css';

interface Props {
  /** Project-relative path of the image file. */
  path: string;
  /** Monotonic counter for this path's binary doc; a change re-reads the bytes. */
  version: number;
}

interface LoadedImage {
  url: string;
  byteSize: number;
}

interface Dimensions {
  width: number;
  height: number;
}

export default function ImageViewer({ path, version }: Props) {
  // Read the synced bytes as a data URL. A data URL (rather than a blob
  // URL) keeps this a pure computation: nothing to revoke, so StrictMode's
  // double-invoked effects cannot kill the URL before the <img> fetches it.
  // Uploads are capped at FILE_SIZE_LIMITS.MAX_FILE_SIZE, so size is bounded.
  // `version` is a dependency on purpose: it is what makes a re-read happen.
  const image = useMemo<LoadedImage | null>(() => {
    void version;
    const binary = getBinaryFileContent(path);
    if (!binary) return null;
    return { url: binaryToDataUrl(binary.content, binary.mimeType), byteSize: binary.content.byteLength };
  }, [path, version]);

  // Decoded size and failure state, keyed by the URL they describe so a
  // re-read or path change never shows stale values.
  const [loaded, setLoaded] = useState<{ url: string; dimensions: Dimensions | null; failed: boolean } | null>(null);
  const dimensions = loaded && image && loaded.url === image.url ? loaded.dimensions : null;
  const failed = !!(loaded && image && loaded.url === image.url && loaded.failed);

  return (
    <div className="image-viewer" data-testid="image-viewer">
      <div className="image-viewer-canvas">
        {image && !failed && (
          <img
            className="image-viewer-img"
            src={image.url}
            alt={path}
            onLoad={(e) => {
              const el = e.currentTarget;
              setLoaded({
                url: image.url,
                dimensions: { width: el.naturalWidth, height: el.naturalHeight },
                failed: false,
              });
            }}
            onError={() => setLoaded({ url: image.url, dimensions: null, failed: true })}
          />
        )}
        {image && failed && (
          <div className="image-viewer-message" role="status">
            This image could not be displayed.
          </div>
        )}
        {!image && (
          <div className="image-viewer-message" role="status">
            Loading image…
          </div>
        )}
      </div>
      <div className="image-viewer-status">
        <span className="image-viewer-path qh-truncate">{path}</span>
        {image && (
          <span className="image-viewer-meta">
            {dimensions ? `${dimensions.width} × ${dimensions.height} · ` : ''}
            {formatByteSize(image.byteSize)}
          </span>
        )}
      </div>
    </div>
  );
}
