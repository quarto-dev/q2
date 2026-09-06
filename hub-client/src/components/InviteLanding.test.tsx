/**
 * Tests for InviteLanding (bd-fxdcxbpq) — the unified landing card for
 * collection (#/join-collection/…) and project (#/share/…) invites.
 *
 * Pins the card contract from design_handoff_invite_landing/README.md:
 * kicker → inviter line → title → payload preview → explainer → single CTA,
 * with the CTA as the last element and no identity form. Copy is exact per
 * the section-3a mocks (as amended: no footnote below the CTA).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import InviteLanding from './InviteLanding';
import type { CollectionInvitePreview, ProjectInvitePreview } from '../utils/invitePreview';

afterEach(cleanup);

const collectionPreview: CollectionInvitePreview = {
  kind: 'collection',
  projects: [
    { name: 'Quarterly report', topFiles: ['report.qmd'], fileCount: 12, contributorInitials: ['CS', 'JL'] },
    { name: 'Methods paper', topFiles: ['paper.qmd'], fileCount: 7, contributorInitials: ['JL'] },
  ],
  totalProjects: 4,
  memberFirstNames: ['Carlos', 'Jenny', 'Mine'],
};

const projectPreview: ProjectInvitePreview = {
  kind: 'project',
  fileName: 'report.qmd',
  topFiles: ['figures/', 'data.csv'],
  fileCount: 12,
  contributorInitials: ['CS', 'JL'],
};

type Props = Parameters<typeof InviteLanding>[0];

/**
 * Stand-in for the provider-rendered Google sign-in button (GIS renders the
 * real one in an iframe; the hub only accepts GIS-minted credentials, so the
 * signed-out CTA is always the provider's node — see plan decision 4).
 */
const fakeSignInCta = <button type="button">Continue with Google</button>;

function renderLanding(overrides: Partial<Props> = {}) {
  const onCta = vi.fn();
  const props: Props = {
    kind: 'collection',
    inviter: 'Carlos Scheidegger',
    title: 'Team docs',
    signedIn: false,
    signInCta: fakeSignInCta,
    joinState: 'idle',
    onCta,
    ...overrides,
  };
  const utils = render(<InviteLanding {...props} />);
  return { onCta, ...utils };
}

describe('InviteLanding card anatomy', () => {
  it('collection invite shows the collection kicker, inviter line, and title', () => {
    renderLanding({ preview: collectionPreview });
    expect(screen.getByText('COLLECTION INVITATION')).toBeTruthy();
    expect(screen.getByText('Carlos Scheidegger')).toBeTruthy();
    expect(screen.getByText(/invited you to\b/)).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Team docs' })).toBeTruthy();
  });

  it('project invite shows the project kicker and "invited you to edit"', () => {
    renderLanding({
      kind: 'project',
      title: 'Quarterly report',
      preview: projectPreview,
    });
    // A #/share/ link grants the whole project, so it is a project
    // invitation even though it opens at one file.
    expect(screen.getByText('PROJECT INVITATION')).toBeTruthy();
    expect(screen.getByText(/invited you to edit/)).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Quarterly report' })).toBeTruthy();
  });

  it('always renders the explainer block', () => {
    renderLanding();
    expect(screen.getByText('New to Quarto Hub?')).toBeTruthy();
    expect(
      screen.getByText(/where teams write Quarto documents together/),
    ).toBeTruthy();
    expect(screen.getByText(/Nothing to install\./)).toBeTruthy();
  });

  it('renders no name input, no color swatches, and nothing after the CTA', () => {
    const { container } = renderLanding({ preview: collectionPreview });
    expect(container.querySelector('input')).toBeNull();
    expect(container.querySelector('.qh-swatch')).toBeNull();
    // The CTA is the last element of the card: no footnote below it.
    const card = screen.getByTestId('invite-landing-card');
    const last = card.lastElementChild;
    expect(last).not.toBeNull();
    expect(last!.querySelector('button') ?? last).toBe(
      screen.getByRole('button', { name: /join|open|continue/i }),
    );
  });
});

describe('InviteLanding payload preview', () => {
  it('collection preview lists project names, mono file summaries, and the more-projects line', () => {
    renderLanding({ preview: collectionPreview });
    expect(screen.getByText('Quarterly report')).toBeTruthy();
    expect(screen.getByText('Methods paper')).toBeTruthy();
    expect(screen.getByText(/report\.qmd · 12 files/)).toBeTruthy();
    expect(screen.getByText(/paper\.qmd · 7 files/)).toBeTruthy();
    expect(
      screen.getByText('+ 2 more projects · Carlos, Jenny and Mine work here'),
    ).toBeTruthy();
  });

  it('project preview lists the contents, opened file first, with a pluralized total', () => {
    renderLanding({ kind: 'project', title: 'Quarterly report', preview: projectPreview });
    expect(
      screen.getByText('report.qmd · figures/ · data.csv · 12 files'),
    ).toBeTruthy();
  });

  it('project preview omits the payload box when the project holds only the invited file', () => {
    // Nothing to list, and the title already names the project — a box
    // containing just "1 file" reads as an empty placeholder.
    renderLanding({
      kind: 'project',
      title: 'Meeting notes',
      preview: { kind: 'project', fileName: 'notes.qmd', topFiles: [], fileCount: 1, contributorInitials: ['CS'] },
    });
    expect(screen.queryByTestId('invite-payload-preview')).toBeNull();
    expect(screen.queryByText(/1 files?/)).toBeNull();
    // The card still reads as a complete invitation.
    expect(screen.getByText('PROJECT INVITATION')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Meeting notes' })).toBeTruthy();
  });

  it('collection rows pluralize a single-file project', () => {
    renderLanding({
      preview: {
        kind: 'collection',
        projects: [{ name: 'Notes', topFiles: ['notes.qmd'], fileCount: 1, contributorInitials: ['CS'] }],
        totalProjects: 1,
        memberFirstNames: ['Carlos'],
      },
    });
    expect(screen.getByText('notes.qmd · 1 file')).toBeTruthy();
  });

  it('renders no document thumbnail (preview payloads never carry content)', () => {
    const { container } = renderLanding({
      kind: 'project',
      title: 'Quarterly report',
      preview: projectPreview,
    });
    expect(container.querySelector('.il-doc-thumb')).toBeNull();
    expect(container.querySelector('.il-doc-chip')).toBeNull();
  });

  it('skips the payload block entirely when preview is absent (legacy links)', () => {
    renderLanding();
    expect(screen.queryByTestId('invite-payload-preview')).toBeNull();
  });
});

describe('InviteLanding CTA matrix', () => {
  it('signed out: renders the provider sign-in node, not a join/open button', () => {
    renderLanding({ preview: collectionPreview });
    expect(screen.getByRole('button', { name: 'Continue with Google' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /join|open/i })).toBeNull();
  });

  it('signed out + collection: "Join to collaborate on <collection>" leads into the sign-in button', () => {
    renderLanding({ preview: collectionPreview });
    expect(screen.getByText('Join to collaborate on Team docs')).toBeTruthy();
  });

  it('signed out + project: the lead-in names the project, never the file it opens at', () => {
    renderLanding({ kind: 'project', title: 'Quarterly report', preview: projectPreview });
    expect(screen.getByText('Join to collaborate on Quarterly report')).toBeTruthy();
    expect(screen.queryByText(/Join to collaborate on report\.qmd/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Continue with Google' })).toBeTruthy();
  });

  it('signed out + project legacy (no preview): lead-in still names the project', () => {
    renderLanding({ kind: 'project', title: 'Quarterly report' });
    expect(screen.getByText('Join to collaborate on Quarterly report')).toBeTruthy();
  });

  it('signed in + collection: "Open <collection name>" (no sign-in friction, so the verb is just open)', () => {
    renderLanding({ signedIn: true, preview: collectionPreview });
    expect(screen.getByRole('button', { name: 'Open Team docs' })).toBeTruthy();
  });

  it('signed in + document: "Open <title>"', () => {
    renderLanding({
      kind: 'project',
      title: 'Quarterly report',
      signedIn: true,
      preview: projectPreview,
    });
    expect(
      screen.getByRole('button', { name: 'Open Quarterly report' }),
    ).toBeTruthy();
  });

  it('signed in legacy links (no preview) use generic CTA text', () => {
    renderLanding({ signedIn: true });
    expect(screen.getByRole('button', { name: 'Open collection' })).toBeTruthy();
    cleanup();
    renderLanding({ kind: 'project', title: 'Quarterly report', signedIn: true });
    expect(screen.getByRole('button', { name: 'Open project' })).toBeTruthy();
  });

  it('there is exactly one button in the card', () => {
    renderLanding({ preview: collectionPreview, error: null });
    expect(screen.getAllByRole('button')).toHaveLength(1);
  });

  it('clicking the CTA fires onCta once', () => {
    const { onCta } = renderLanding({ signedIn: true, preview: collectionPreview });
    screen.getByRole('button', { name: 'Open Team docs' }).click();
    expect(onCta).toHaveBeenCalledTimes(1);
  });

  it('the CTA is disabled and shows a busy label while joining', () => {
    renderLanding({ signedIn: true, preview: collectionPreview, joinState: 'joining' });
    const button = screen.getByRole('button');
    expect(button.hasAttribute('disabled')).toBe(true);
    expect(button.textContent).toMatch(/Opening/);
  });

  it('renders an error inside the card when one is passed', () => {
    renderLanding({ signedIn: true, error: 'This collection is not available.' });
    expect(screen.getByText('This collection is not available.')).toBeTruthy();
  });
});
