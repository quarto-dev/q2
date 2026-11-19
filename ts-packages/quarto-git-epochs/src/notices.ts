/**
 * Every notice text of the git-epochs epic, one exported constant each. This file is the only place the wording
 * lives: clients import the constants and never restate them. Templates use `{n}`, `{path}`, and `{name}`
 * placeholders; fill them with {@link formatNotice}. The notices say "project owner", not "host".
 */

const REPLACED_SUFFIX = "because the project owner's checked-out version changed.";

/** Fill `{key}` placeholders in a notice template. */
export function formatNotice(template: string, vars: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (whole, key: string) =>
    Object.prototype.hasOwnProperty.call(vars, key) ? String(vars[key]) : whole,
  );
}

// Used by GE-7 (browser clients).
export const PAUSED_BANNER =
  'Paused for git: edits to files the project owner rewrites may be set aside';
export const BRANCHES_STAY = '{n} branches of this file stay with the previous version';

// Used by GE-9 (extension follower modes).
export const GIT_OP_EDITS_DROPPED = 'Edits made during a git operation were not kept.';
export const EDITOR_ONLY_NO_FLIP = 'The project owner has not switched versions yet. Keep waiting?';
export const EDITOR_ONLY_NO_FLIP_ACTIONS = ['Keep waiting', 'Dismiss'];

// Used by GE-10 (extension clone mode).
export const FILES_DETACHED =
  '{n} files are detached: your checked-out version differs from the shared one. Your edits to them no longer reach the project.';
export const FILES_DETACHED_ACTIONS = ['Adopt shared version', 'Stay detached'];
export const FILES_JOINED_LIVE =
  '{n} files joined live edits from collaborators that are not committed to git.';
export const FILES_JOINED_LIVE_ACTIONS = ['Use my checkout instead'];
export const ADOPT_CONFIRM = 'This file has uncommitted changes. Replace it with the shared version?';
export const DOC_SUPERSEDED = 'Your doc was superseded';

// Produced by the NoticeAggregator in swap.ts.
export const REPLACED_THIS_FILE = `This file was replaced ${REPLACED_SUFFIX}`;
export const REPLACED_ONE_FILE = `The file {path} was replaced ${REPLACED_SUFFIX}`;
export const REPLACED_MANY_FILES = `{n} files were replaced ${REPLACED_SUFFIX}`;
export const STRANDED_EDITS = '{n} of your recent edits stayed with the previous version.';

// The moved presence message (moved.ts).
export const MOVED_NAMED = '{name} switched versions';
export const MOVED_ANONYMOUS = 'This person switched versions';

// Names fixed by the GE-1 contract ("Swap events and notices"); aliases of the texts above.
export const NOTICE_SWITCHED_VERSIONS = REPLACED_THIS_FILE;
export const NOTICE_STRANDED_EDITS = STRANDED_EDITS;
export const NOTICE_UNAVAILABLE = "The project owner's version of this file is not available yet.";
export const NOTICE_DETACHED = FILES_DETACHED;
export const NOTICE_JOINED_LIVE = FILES_JOINED_LIVE;
export const NOTICE_EDITS_NOT_KEPT = GIT_OP_EDITS_DROPPED;
