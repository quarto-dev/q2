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
    expect(screen.getByText(/invites you to collaborate on/)).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Team docs' })).toBeTruthy();
  });

  it('project invite shows the project kicker and the same collaborate wording', () => {
    renderLanding({
      kind: 'project',
      title: 'Quarterly report',
      preview: projectPreview,
    });
    // A #/share/ link grants the whole project, so it is a project
    // invitation even though it opens at one file.
    expect(screen.getByText('PROJECT INVITATION')).toBeTruthy();
    // Both kinds read identically: only the kicker and payload differ.
    expect(screen.getByText(/invites you to collaborate on/)).toBeTruthy();
    expect(screen.queryByText(/invited you to edit/)).toBeNull();
    expect(screen.getByRole('heading', { name: 'Quarterly report' })).toBeTruthy();
  });

  it('names Quarto Hub on the kicker line, right-aligned', () => {
    // Without this the card never said where you were being invited to:
    // the destination was only implied by a footnote aimed at newcomers.
    const { container } = renderLanding();
    const card = screen.getByTestId('invite-landing-card');
    const header = container.querySelector('.il-header')!;
    const kicker = container.querySelector('.il-kicker')!;
    const lockup = container.querySelector('.il-lockup')!;
    expect(header).not.toBeNull();
    expect(lockup.textContent).toContain('Quarto Hub');
    // One row at the top of the card: kicker first, lockup opposite it.
    expect(card.firstElementChild).toBe(header);
    expect(header.children).toHaveLength(2);
    expect(header.firstElementChild).toBe(kicker);
    expect(header.lastElementChild).toBe(lockup);
  });

  it('the footnote reads "New to Quarto Hub? Learn more." in both signed-in states', () => {
    for (const signedIn of [false, true]) {
      renderLanding({ signedIn, signInCta: fakeSignInCta });
      expect(screen.getByText(/New to Quarto Hub\?/)).toBeTruthy();
      const link = screen.getByRole('link', { name: 'Learn more' });
      // Points at the real Quarto Hub site, not the quarto.org
      // placeholder it shipped with (bd-rh2n4d7q), and opens in a new
      // tab — following it in place abandons the invite, which a
      // recipient may have no easy way back to.
      expect(link.getAttribute('href')).toBe('https://quarto-dev.github.io/quarto-hub/');
      expect(link.getAttribute('target')).toBe('_blank');
      expect(link.getAttribute('rel')).toBe('noopener noreferrer');
      cleanup();
    }
  });

  it('renders no name input and no color swatches', () => {
    const { container } = renderLanding({ preview: collectionPreview });
    expect(container.querySelector('input')).toBeNull();
    expect(container.querySelector('.qh-swatch')).toBeNull();
  });

  it('the explainer footnote sits below the CTA', () => {
    // Departs from the 3a mock, which put the explainer above a
    // last-element CTA: Andrew moved it under the button so the card
    // leads with who/what/act and leaves the pitch as a footnote.
    renderLanding({ signedIn: true, preview: collectionPreview });
    const card = screen.getByTestId('invite-landing-card');
    const actions = card.querySelector('.il-actions')!;
    const explainer = card.querySelector('.il-explainer')!;
    expect(actions).not.toBeNull();
    expect(explainer).not.toBeNull();
    expect(
      actions.compareDocumentPosition(explainer) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(card.lastElementChild).toBe(explainer);
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

  it('a collection row with no cached summary names the project and claims no count', () => {
    // fileCount 0 means the sender had no cached summary for that
    // project, not that it is empty — every project is scaffolded with
    // at least two files, so "0 files" would be a plain lie.
    renderLanding({
      preview: {
        kind: 'collection',
        projects: [
          { name: 'Never opened here', topFiles: [], fileCount: 0, contributorInitials: [] },
          { name: 'Quarterly report', topFiles: ['report.qmd'], fileCount: 12, contributorInitials: ['CS'] },
        ],
        totalProjects: 2,
        memberFirstNames: ['Carlos'],
      },
    });
    expect(screen.getByText('Never opened here')).toBeTruthy();
    expect(screen.queryByText(/0 files?/)).toBeNull();
    // The project that does have a summary still shows one.
    expect(screen.getByText('report.qmd · 12 files')).toBeTruthy();
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

  it('signed out: the sign-in button stands alone, with no lead-in line above it', () => {
    // The inviter line already says "<name> invites you to collaborate
    // on <title>", so a "Join to collaborate on …" line above the button
    // repeated it — and, sitting directly above a real button, read like
    // one itself.
    const { container } = renderLanding({ preview: collectionPreview });
    expect(screen.queryByText(/Join to collaborate on/)).toBeNull();
    expect(container.querySelector('.il-signin-lead')).toBeNull();
    const actions = container.querySelector('.il-actions')!;
    expect(actions.children).toHaveLength(1);
  });

  it('signed out + project: still no lead-in, for either a rich or a legacy link', () => {
    renderLanding({ kind: 'project', title: 'Quarterly report', preview: projectPreview });
    expect(screen.queryByText(/Join to collaborate on/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Continue with Google' })).toBeTruthy();
    cleanup();
    renderLanding({ kind: 'project', title: 'Quarterly report' });
    expect(screen.queryByText(/Join to collaborate on/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Continue with Google' })).toBeTruthy();
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
