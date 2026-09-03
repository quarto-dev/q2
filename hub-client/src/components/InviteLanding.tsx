/**
 * InviteLanding — the unified landing card for collection and project
 * invites (bd-fxdcxbpq, design_handoff_invite_landing/).
 *
 * A `#/share/…` link grants access to a whole project (the index
 * document) and merely opens at one file, so it is framed as a PROJECT
 * INVITATION throughout; only the collection card speaks of projects
 * inside it.
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
import type { CollectionInvitePreview, ProjectInvitePreview, InvitePreview } from '../utils/invitePreview';
import { generateColorFromId } from '../services/storage/utils';
import { initialsFor } from '../utils/facepile';

export interface InviteLandingProps {
  kind: 'collection' | 'project';
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

/** "report.qmd · 12 files" — the count is pluralized, so never "1 files". */
function fileSummary(topFiles: string[], fileCount: number): string {
  return [...topFiles, `${fileCount} ${fileCount === 1 ? 'file' : 'files'}`].join(' · ');
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

/**
 * A project invite's payload lists what is in the project — the file the
 * invite opens at, then other paths, then the total — which is the
 * project-level analogue of the collection card listing its projects.
 *
 * With a single file there is nothing to list: the card's title already
 * names the project, so this renders nothing rather than a box holding a
 * lone count (the ruled-paper thumbnail this replaced was decoration —
 * `preview=` never carries document content, so it could only ever draw
 * an empty page).
 */
function ProjectPayload({ preview }: { preview: ProjectInvitePreview }) {
  if (preview.fileCount <= 1) return null;
  const files = [preview.fileName, ...preview.topFiles].filter(Boolean);
  return (
    <div className="il-payload" data-testid="invite-payload-preview">
      <div className="il-payload-row">
        <span className="il-file-summary mono">{fileSummary(files, preview.fileCount)}</span>
        <Facepile initials={preview.contributorInitials} />
      </div>
    </div>
  );
}

// Signed-in CTA: no sign-in friction left, so the verb is simply "Open"
// for both kinds (the join happens implicitly for collections). The
// "join to collaborate" framing is reserved for the signed-out state,
// where the user is about to go through Google sign-in for the first time.
function ctaLabel(props: InviteLandingProps): string {
  const { kind, joinState, preview, title } = props;
  if (joinState === 'joining') {
    return 'Opening…';
  }
  if (kind === 'collection') {
    return preview ? `Open ${title}` : 'Open collection';
  }
  return preview ? `Open ${title}` : 'Open project';
}

export default function InviteLanding(props: InviteLandingProps) {
  const { kind, inviter, title, preview, signedIn, signInCta, joinState, ctaDisabled, error, onCta } = props;
  const busy = joinState === 'joining';

  return (
    <div className="il-wrap">
      <div className="il-card" data-testid="invite-landing-card">
        <div className="il-kicker">
          {kind === 'collection' ? 'COLLECTION INVITATION' : 'PROJECT INVITATION'}
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
        {preview?.kind === 'project' && <ProjectPayload preview={preview} />}
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
            <>
              {/* The invite is to the project (or collection) the title
                  names — never to one file, even though a project invite
                  opens at one. */}
              <div className="il-signin-lead">Join to collaborate on {title}</div>
              {signInCta}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
