/** Tool checks and the engine setup invocations (DM10). The launcher runs the commands; this module only decides them. */

export type Which = (tool: 'R' | 'rv' | 'uv' | 'q2') => boolean;
export interface Cmd {
  cmd: string;
  args: string[];
  cwd: string;
}

export function setupCommands(d: { root: string; journal: string }): Cmd[] {
  return [
    { cmd: 'rv', args: ['sync'], cwd: d.root },
    { cmd: 'rv', args: ['library'], cwd: d.root },
    { cmd: 'uv', args: ['sync'], cwd: d.journal },
  ];
}

const ADVICE: Record<string, string> = {
  R: 'R is missing: install it with `brew install --cask r` (or from https://cloud.r-project.org).',
  rv: 'rv is missing: install it with `curl -sSL https://a2-ai.github.io/rv-docs/install.sh | bash` (see https://a2-ai.github.io/rv-docs/).',
  uv: 'uv is missing: install it with `brew install uv`.',
  q2: 'q2 is missing from PATH: build it with `cargo build --bin q2` and put target/debug on PATH.',
};

export function missingToolAdvice(which: Which): string[] {
  return (['R', 'rv', 'uv', 'q2'] as const).filter((t) => !which(t)).map((t) => ADVICE[t]!);
}
