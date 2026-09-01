/**
 * Tests for InviteLanding (bd-fxdcxbpq) — the unified landing card for
 * collection (#/join-collection/…) and document (#/share/…) invites.
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
import type { CollectionInvitePreview, DocumentInvitePreview } from '../utils/invitePreview';

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

const documentPreview: DocumentInvitePreview = {
  kind: 'document',
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

  it('document invite shows the document kicker and "invited you to edit"', () => {
    renderLanding({
      kind: 'document',
      title: 'Quarterly report',
      preview: documentPreview,
    });
    expect(screen.getByText('DOCUMENT INVITATION')).toBeTruthy();
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

  it('document preview shows the filename chip and file summary row', () => {
    renderLanding({ kind: 'document', title: 'Quarterly report', preview: documentPreview });
    expect(screen.getByText('report.qmd')).toBeTruthy();
    expect(screen.getByText(/figures\/ · data\.csv · 12 files/)).toBeTruthy();
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

  it('signed out + document: same provider sign-in node', () => {
    renderLanding({ kind: 'document', title: 'Quarterly report', preview: documentPreview });
    expect(screen.getByRole('button', { name: 'Continue with Google' })).toBeTruthy();
  });

  it('signed in + collection: "Join <collection name>" (the invite is to the collection, not a document)', () => {
    renderLanding({ signedIn: true, preview: collectionPreview });
    expect(screen.getByRole('button', { name: 'Join Team docs' })).toBeTruthy();
  });

  it('signed in + document: "Open <title>"', () => {
    renderLanding({
      kind: 'document',
      title: 'Quarterly report',
      signedIn: true,
      preview: documentPreview,
    });
    expect(
      screen.getByRole('button', { name: 'Open Quarterly report' }),
    ).toBeTruthy();
  });

  it('signed in legacy links (no preview) use generic CTA text', () => {
    renderLanding({ signedIn: true });
    expect(screen.getByRole('button', { name: 'Join collection' })).toBeTruthy();
    cleanup();
    renderLanding({ kind: 'document', title: 'Quarterly report', signedIn: true });
    expect(screen.getByRole('button', { name: 'Open document' })).toBeTruthy();
  });

  it('there is exactly one button in the card', () => {
    renderLanding({ preview: collectionPreview, error: null });
    expect(screen.getAllByRole('button')).toHaveLength(1);
  });

  it('clicking the CTA fires onCta once', () => {
    const { onCta } = renderLanding({ signedIn: true, preview: collectionPreview });
    screen.getByRole('button', { name: 'Join Team docs' }).click();
    expect(onCta).toHaveBeenCalledTimes(1);
  });

  it('the CTA is disabled and shows a busy label while joining', () => {
    renderLanding({ signedIn: true, preview: collectionPreview, joinState: 'joining' });
    const button = screen.getByRole('button');
    expect(button.hasAttribute('disabled')).toBe(true);
    expect(button.textContent).toMatch(/Joining/);
  });

  it('renders an error inside the card when one is passed', () => {
    renderLanding({ signedIn: true, error: 'This collection is not available.' });
    expect(screen.getByText('This collection is not available.')).toBeTruthy();
  });
});
