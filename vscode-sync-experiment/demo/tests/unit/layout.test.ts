import { describe, expect, it } from 'vitest';
import { computeLayout, mainMonitorVisibleFrame } from '../../src/layout';

describe('layout (DM5)', () => {
  it('the main monitor frame starts below the menu bar', () => {
    expect(mainMonitorVisibleFrame({ width: 1920, height: 1280, menuBar: 30 })).toEqual({ left: 0, top: 30, width: 1920, height: 1250 });
  });

  it('with a guest terminal: two columns, editor and terminal split 3:1 in each', () => {
    const l = computeLayout({ left: 0, top: 30, width: 1920, height: 1250 }, { guestTerminal: true });
    expect(l.hostEditor).toEqual({ left: 0, top: 30, width: 960, height: 938 });
    expect(l.hostTerminal).toEqual({ left: 0, top: 968, width: 960, height: 312 });
    expect(l.guestEditor).toEqual({ left: 960, top: 30, width: 960, height: 938 });
    expect(l.guestTerminal).toEqual({ left: 960, top: 968, width: 960, height: 312 });
  });

  it('without a guest terminal the guest editor takes the whole right column', () => {
    const l = computeLayout({ left: 0, top: 30, width: 1920, height: 1250 }, { guestTerminal: false });
    expect(l.guestEditor).toEqual({ left: 960, top: 30, width: 960, height: 1250 });
    expect(l.guestTerminal).toBeUndefined();
  });

  it('honors a screen that is not at the origin (--screen)', () => {
    const l = computeLayout({ left: 1920, top: 0, width: 1000, height: 800 }, { guestTerminal: false });
    expect(l.hostEditor).toEqual({ left: 1920, top: 0, width: 500, height: 600 });
    expect(l.hostTerminal).toEqual({ left: 1920, top: 600, width: 500, height: 200 });
    expect(l.guestEditor).toEqual({ left: 2420, top: 0, width: 500, height: 800 });
  });
});
