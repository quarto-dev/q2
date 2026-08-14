/**
 * EphemeralSessionBanner — the persistent banner for `q2 preview
 * --ui editor` sessions started without --allow-edit: edits sync live
 * to everyone connected but are never written to the host's files.
 *
 * Editor renders this when App wires `sessionEphemeral` (the serving
 * server's /api/preview/config reports `allowEdit === false`; a --join
 * guest reads the host's value through the tunnel). The copy is fixed
 * — nothing here is interpolated from server data.
 */
export default function EphemeralSessionBanner() {
  return (
    <div
      className="ephemeral-session-banner"
      role="status"
      title="Started without --allow-edit: edits sync live to everyone connected but are never written to the project's files. Restart the preview with --allow-edit to persist them."
    >
      Ephemeral session — edits won't be saved to disk
    </div>
  );
}
