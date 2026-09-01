/**
 * InviteLanding — the unified landing card for collection and document
 * invites (bd-fxdcxbpq, design_handoff_invite_landing/).
 *
 * One layout for both invite kinds: kicker → inviter line → title →
 * display-only payload preview → what-is-QuartoHub explainer → a single CTA,
 * which is always the last element of the card. Signed-out users get a
 * Google CTA (the caller wires it to the auth flow); signed-in users get a
 * one-click join/open. The card renders no identity form — identity comes
 * from the Google account.
 */

import './InviteLanding.css';
import type { ReactNode } from 'react';
import type { CollectionInvitePreview, DocumentInvitePreview, InvitePreview } from '../utils/invitePreview';
import { generateColorFromId } from '../services/storage/utils';
import { initialsFor } from '../utils/facepile';

export interface InviteLandingProps {
  kind: 'collection' | 'document';
  /** Display name of the person who sent the invite. */
  inviter: string;
  /** Collection or project name. */
  title: string;
  /** Display-only preview payload; absent on legacy links. */
  preview?: InvitePreview;
  signedIn: boolean;
  /**
   * Rendered as the CTA when signed out. Always the auth provider's own
   * sign-in node (GIS with text="continue_with"): the hub only accepts
   * GIS-minted credentials, so a custom Google button cannot work.
   */
  signInCta?: ReactNode;
  /** Name of the post-join start target, for CTA copy (collection only). */
  startName?: string;
  joinState: 'idle' | 'joining';
  /** Disable the signed-in CTA (e.g. while the personal root connects). */
  ctaDisabled?: boolean;
  error?: string | null;
  onCta: () => void;
}

/** "Carlos" / "Carlos and Jenny" / "Carlos, Jenny and Mine". */
function formatNameList(names: string[]): string {
  if (names.length <= 1) return names[0] ?? '';
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

function Facepile({ initials }: { initials: string[] }) {
  if (initials.length === 0) return null;
  return (
    <span className="il-facepile" aria-hidden="true">
      {initials.map((i, idx) => (
        <span key={`${i}-${idx}`} className="il-face" style={{ backgroundColor: generateColorFromId(i) }}>
          {i}
        </span>
      ))}
    </span>
  );
}

function fileSummary(topFiles: string[], fileCount: number): string {
  return [...topFiles, `${fileCount} files`].join(' · ');
}

function CollectionPayload({ preview }: { preview: CollectionInvitePreview }) {
  const extra = preview.totalProjects - preview.projects.length;
  const footerParts = [
    ...(extra > 0 ? [`+ ${extra} more project${extra === 1 ? '' : 's'}`] : []),
    ...(preview.memberFirstNames.length > 0
      ? [`${formatNameList(preview.memberFirstNames)} work here`]
      : []),
  ];
  return (
    <div className="il-payload" data-testid="invite-payload-preview">
      {preview.projects.map((p) => (
        <div key={p.name} className="il-payload-row">
          <span className="il-project">
            <span className="il-project-name">{p.name}</span>
            <span className="il-file-summary mono">{fileSummary(p.topFiles, p.fileCount)}</span>
          </span>
          <Facepile initials={p.contributorInitials} />
        </div>
      ))}
      {footerParts.length > 0 && <div className="il-payload-footer">{footerParts.join(' · ')}</div>}
    </div>
  );
}

function DocumentPayload({ preview }: { preview: DocumentInvitePreview }) {
  return (
    <div className="il-payload" data-testid="invite-payload-preview">
      <div className="il-doc-thumb">
        <span className="il-doc-chip mono">{preview.fileName}</span>
      </div>
      <div className="il-payload-row">
        <span className="il-file-summary mono">{fileSummary(preview.topFiles, preview.fileCount)}</span>
        <Facepile initials={preview.contributorInitials} />
      </div>
    </div>
  );
}

function ctaLabel(props: InviteLandingProps): string {
  const { kind, joinState, preview, startName, title } = props;
  if (joinState === 'joining') {
    return kind === 'collection' ? 'Joining…' : 'Opening…';
  }
  if (kind === 'collection') {
    if (startName) return `Join and open ${startName}`;
    return preview ? `Join ${title}` : 'Join collection';
  }
  return preview ? `Open ${title}` : 'Open document';
}

export default function InviteLanding(props: InviteLandingProps) {
  const { kind, inviter, title, preview, signedIn, signInCta, joinState, ctaDisabled, error, onCta } = props;
  const busy = joinState === 'joining';

  return (
    <div className="il-wrap">
      <div className="il-card" data-testid="invite-landing-card">
        <div className="il-kicker">
          {kind === 'collection' ? 'COLLECTION INVITATION' : 'DOCUMENT INVITATION'}
        </div>
        <div className="il-inviter">
          <span
            className="il-inviter-avatar"
            style={{ backgroundColor: generateColorFromId(inviter) }}
            aria-hidden="true"
          >
            {initialsFor(inviter)}
          </span>
          <span>
            <strong>{inviter}</strong>
            {kind === 'collection' ? ' invited you to' : ' invited you to edit'}
          </span>
        </div>
        <h1 className="il-title">{title}</h1>
        {preview?.kind === 'collection' && <CollectionPayload preview={preview} />}
        {preview?.kind === 'document' && <DocumentPayload preview={preview} />}
        <div className="il-explainer">
          <img className="il-explainer-logo" src="/quarto-icon.svg" alt="" />
          <span>
            <strong>New to Quarto Hub?</strong> It's where teams write Quarto documents together
            — live, in the browser. Nothing to install.
          </span>
        </div>
        {error && (
          <div className="qh-error" role="alert">
            {error}
          </div>
        )}
        <div className="il-actions">
          {signedIn ? (
            <button type="button" className="qh-btn primary" disabled={busy || ctaDisabled} onClick={onCta}>
              {ctaLabel(props)}
            </button>
          ) : (
            signInCta
          )}
        </div>
      </div>
    </div>
  );
}
