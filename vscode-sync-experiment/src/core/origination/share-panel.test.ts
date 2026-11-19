import { parseProjectRef } from '@quarto/share-url';
import { describe, expect, it } from 'vitest';
import { shareForms } from './share-panel';

describe('shareForms', () => {
  const info = { server: 'wss://sync.automerge.org', indexDocId: '3RFyJzsLsZ7MsbWCQz2kuoeRRaTX', name: 'My Site', file: 'index.qmd' };

  it('the share URL has all four parts and round-trips through the parser', () => {
    const url = shareForms(info)[0]!.text;
    expect(url.startsWith('https://')).toBe(true);
    expect(parseProjectRef(url)).toMatchObject({ project: info.indexDocId, server: info.server, name: 'My Site', file: 'index.qmd' });
  });

  it('the command is the exact q2 preview invocation', () => {
    expect(shareForms(info)[1]!.text).toBe('q2 preview --hub-url wss://sync.automerge.org --project 3RFyJzsLsZ7MsbWCQz2kuoeRRaTX --ui editor --allow-edit');
  });

  it('the Join form names the server and the id', () => {
    expect(shareForms(info)[2]!.text).toBe('server: wss://sync.automerge.org\nproject: 3RFyJzsLsZ7MsbWCQz2kuoeRRaTX');
  });

  it('a project with no file to name opens index.qmd', () => {
    const { file: _drop, ...rest } = info;
    expect(parseProjectRef(shareForms(rest)[0]!.text).file).toBe('index.qmd');
  });
});
