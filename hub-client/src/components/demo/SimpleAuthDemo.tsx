/**
 * #/demo/simple-auth — a standalone example page. A counter document lives
 * in IndexedDB and only reaches the sync server while signed in. Each time
 * the counter changes, the document's waves light up; each time the server
 * reports that it holds our latest heads, the tower's waves light up. Auth is
 * handled here directly against the hub's HTTP endpoints.
 */

import { useCallback, useEffect, useState } from 'react';
import { GoogleLogin, GoogleOAuthProvider } from '@react-oauth/google';
import { Repo, isValidAutomergeUrl, type DocHandle } from '@automerge/automerge-repo';
import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import SyncDiagram from './SyncDiagram';
import DemoFrame from './DemoFrame';
import './SimpleAuthDemo.css';

// The hub that serves this page (e.g. local prod at http://127.0.0.1:8080).
const HUB_URL = window.location.origin;
const SYNC_SERVER_URL = `${HUB_URL.replace(/^http/, 'ws')}/ws`;
const GOOGLE_CLIENT_ID = import.meta.env.VITE_GOOGLE_CLIENT_ID as string | undefined;
const DOC_URL_KEY = 'q2-demo-simple-auth-doc';
const LAST_ACK_KEY = 'q2-demo-simple-auth-last-ack';
const LAST_CHANGE_KEY = 'q2-demo-simple-auth-last-change';
const TICK_MS = 5000;
const FLASH_MS = 1000;

interface CounterDoc { count: number }

// ── Automerge ───────────────────────────────────────────────────────────

// One repo for the page's lifetime (module scope survives React's dev
// double-mount). Storage only; the WebSocket adapter follows sign-in state.
let repo: Repo | undefined;
let docPromise: Promise<DocHandle<CounterDoc>> | undefined;
let ws: BrowserWebSocketClientAdapter | null = null;

function getRepo(): Repo {
  repo ??= new Repo({ storage: new IndexedDBStorageAdapter(), network: [] });
  return repo;
}

function setNetworkEnabled(allowed: boolean): void {
  const net = getRepo().networkSubsystem;
  if (allowed && ws === null) {
    ws = new BrowserWebSocketClientAdapter(SYNC_SERVER_URL);
    net.addNetworkAdapter(ws);
  } else if (!allowed && ws !== null) {
    ws.disconnect();
    net.removeNetworkAdapter(ws);
    ws = null;
  }
}

/** Reopen the counter doc remembered in localStorage, or create a new one. */
async function openDoc(): Promise<DocHandle<CounterDoc>> {
  const saved = localStorage.getItem(DOC_URL_KEY);
  if (saved !== null && isValidAutomergeUrl(saved)) {
    try {
      return await getRepo().find<CounterDoc>(saved);
    } catch {
      // Storage was cleared; fall through and start over.
    }
  }
  const handle = getRepo().create<CounterDoc>({ count: 0 });
  localStorage.setItem(DOC_URL_KEY, handle.url);
  return handle;
}

/** Remember a timestamp across reloads (sign-in round-trips through Google). */
function saveTime(key: string, time: number): number {
  localStorage.setItem(key, String(time));
  return time;
}

function loadTime(key: string): number | null {
  const saved = Number(localStorage.getItem(key));
  return saved > 0 ? saved : null;
}

// ── Auth ────────────────────────────────────────────────────────────────

/** Email of the signed-in user per the hub's session cookie, or null. */
async function fetchIdentity(): Promise<string | null> {
  const res = await fetch(`${HUB_URL}/auth/me`, { credentials: 'same-origin' });
  if (!res.ok) return null;
  const me = await res.json() as { email: string };
  return me.email;
}

async function signOut(): Promise<void> {
  await fetch(`${HUB_URL}/auth/logout`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'X-Requested-With': 'XMLHttpRequest' },
  });
}

/** Google sign-in button; the credential lands at the hub's callback. */
function SignInButton() {
  const [nonce, setNonce] = useState<string | null>(null);

  // The hub pairs the nonce with a cookie it checks on callback.
  useEffect(() => {
    void fetch(`${HUB_URL}/auth/nonce`, { credentials: 'same-origin' })
      .then((res) => res.json())
      .then((body: { nonce: string }) => setNonce(body.nonce))
      .catch(() => setNonce(null));
  }, []);

  if (GOOGLE_CLIENT_ID === undefined || nonce === null) return null;
  return (
    <GoogleOAuthProvider clientId={GOOGLE_CLIENT_ID}>
      <GoogleLogin ux_mode="redirect" login_uri={`${HUB_URL}/auth/callback`} nonce={nonce} onSuccess={() => {}} />
    </GoogleOAuthProvider>
  );
}

// ── Page ────────────────────────────────────────────────────────────────

/** A boolean that stays true for a moment after each call to the trigger. */
function useFlash(): [boolean, () => void] {
  const [lit, setLit] = useState(false);
  useEffect(() => {
    if (!lit) return;
    const timer = setTimeout(() => setLit(false), FLASH_MS);
    return () => clearTimeout(timer);
  }, [lit]);
  return [lit, useCallback(() => setLit(true), [])];
}

export default function SimpleAuthDemo() {
  const [identity, setIdentity] = useState<string | null>(null);
  const [count, setCount] = useState(0);
  const [connected, setConnected] = useState(false);
  const [lastAck, setLastAck] = useState(() => loadTime(LAST_ACK_KEY));
  const [lastChange, setLastChange] = useState(() => loadTime(LAST_CHANGE_KEY) ?? 0);
  const [docLit, flashDoc] = useFlash();
  const [ackLit, flashAck] = useFlash();

  useEffect(() => {
    void fetchIdentity().then(setIdentity).catch(() => setIdentity(null));
  }, []);

  // Connect to the sync server only while signed in.
  useEffect(() => {
    setNetworkEnabled(identity !== null);
  }, [identity]);

  useEffect(() => {
    const net = getRepo().networkSubsystem;
    const onPeer = () => setConnected(true);
    const onPeerGone = () => setConnected(false);
    net.on('peer', onPeer);
    net.on('peer-disconnected', onPeerGone);

    let handle: DocHandle<CounterDoc> | undefined;
    let timer: ReturnType<typeof setInterval> | undefined;
    const onChange = ({ doc }: { doc?: CounterDoc }) => setCount(doc?.count ?? 0);
    const onRemoteHeads = ({ timestamp }: { timestamp: number }) => {
      setLastAck(saveTime(LAST_ACK_KEY, timestamp));
      flashAck();
    };

    docPromise ??= openDoc();
    void docPromise.then((h) => {
      handle = h;
      setCount(h.doc()?.count ?? 0);
      h.on('change', onChange);
      h.on('remote-heads', onRemoteHeads);
      timer = setInterval(() => {
        h.change((d) => { d.count += 1; });
        setLastChange(saveTime(LAST_CHANGE_KEY, Date.now()));
        flashDoc();
      }, TICK_MS);
    });

    return () => {
      clearInterval(timer);
      handle?.off('change', onChange);
      handle?.off('remote-heads', onRemoteHeads);
      net.off('peer', onPeer);
      net.off('peer-disconnected', onPeerGone);
    };
  }, [flashAck, flashDoc]);

  const upToDate = lastAck !== null && lastAck >= lastChange;
  const mode = identity === null
    ? 'Local only — sign in to sync'
    : connected ? 'Connected to sync server' : 'Connecting…';

  return (
    <DemoFrame>
    <div className="sad-page">
      <SyncDiagram count={count} docLit={docLit} ackLit={ackLit} />
      <div className="sad-status">
        {identity !== null && <span>Signed in as {identity}</span>}
        <span>{mode}</span>
        <span>
          <span className={`sad-dot ${ackLit ? 'lit' : upToDate ? 'ok' : 'stale'}`} />
          {lastAck !== null ? `Last synced ${new Date(lastAck).toLocaleTimeString()}` : 'Never synced'}
        </span>
      </div>
      {identity === null
        ? <SignInButton />
        : <button type="button" className="sad-signout" onClick={() => void signOut().then(() => setIdentity(null))}>Sign out</button>}
    </div>
    </DemoFrame>
  );
}
