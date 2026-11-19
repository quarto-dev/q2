/**
 * The terminal window's page (DM2, DM3): xterm.js on the PTY's output, plus the strip. `display` messages are written
 * to the xterm display only; keystrokes go back as `input` messages. The page is plain HTML with the token in the asset
 * URLs; xterm's own files are served by the terminal server (see `terminalAssets`).
 */
import { createRequire } from 'node:module';
import path from 'node:path';

const require = createRequire(import.meta.url);

export function terminalAssets(): Record<string, { file: string; type: string }> {
  const xterm = path.dirname(require.resolve('@xterm/xterm/package.json'));
  const fit = path.dirname(require.resolve('@xterm/addon-fit/package.json'));
  return {
    '/xterm.js': { file: path.join(xterm, 'lib', 'xterm.js'), type: 'text/javascript' },
    '/xterm.css': { file: path.join(xterm, 'css', 'xterm.css'), type: 'text/css' },
    '/addon-fit.js': { file: path.join(fit, 'lib', 'addon-fit.js'), type: 'text/javascript' },
  };
}

export const STRIP_LINES = 3;

export function terminalPageHtml(wsUrl: string, title: string): string {
  const token = new URL(wsUrl).searchParams.get('token')!;
  return `<!doctype html>
<html><head><meta charset="utf-8"><title>${title}</title>
<link rel="stylesheet" href="/xterm.css?token=${token}">
<style>
  html, body { margin: 0; height: 100%; background: #1e1e1e; overflow: hidden; }
  #wrap { display: flex; flex-direction: column; height: 100%; }
  #term { flex: 1; min-height: 0; padding: 4px 6px; }
  #strip { border-top: 1px solid #555; padding: 3px 8px; height: ${STRIP_LINES * 1.35}em; font: 12px/1.35 Menlo, monospace; color: #ccc; overflow: hidden; white-space: pre-wrap; display: none; }
  #strip .dim { opacity: .8; font-style: italic; }
  #strip .red { color: #ff6b6b; font-style: italic; }
</style></head>
<body><div id="wrap"><div id="term"></div><div id="strip" data-testid="strip"></div></div>
<script src="/xterm.js?token=${token}"></script>
<script src="/addon-fit.js?token=${token}"></script>
<script>
  const term = new Terminal({ fontFamily: 'Menlo, monospace', fontSize: 14, cursorBlink: true, convertEol: false, theme: { background: '#1e1e1e' } });
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(document.getElementById('term'));
  const ws = new WebSocket(${JSON.stringify(wsUrl)});
  const send = (m) => ws.readyState === 1 && ws.send(JSON.stringify(m));
  const refit = () => { fit.fit(); send({ type: 'resize', cols: term.cols, rows: term.rows }); };
  window.addEventListener('resize', refit);
  ws.onopen = () => { refit(); term.focus(); };
  term.onData((data) => send({ type: 'input', data }));
  const strip = document.getElementById('strip');
  const esc = (s) => s.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c]));
  const html = (line) => esc(line).replace(/\\x1b\\[2;3m/g, '<span class="dim">').replace(/\\x1b\\[31;3m/g, '<span class="red">').replace(/\\x1b\\[0m/g, '</span>');
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.type === 'output' || m.type === 'display') term.write(m.data);
    else if (m.type === 'strip') {
      strip.style.display = m.lines.length ? 'block' : 'none';
      strip.innerHTML = m.lines.map(html).join('\\n');
      refit();
    }
  };
</script></body></html>`;
}
