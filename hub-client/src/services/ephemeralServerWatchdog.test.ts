import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { startEphemeralServerWatchdog } from './ephemeralServerWatchdog';

describe('startEphemeralServerWatchdog', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('tears down after consecutive failures, then reloads when the server returns', async () => {
    let healthy = true;
    const teardown = vi.fn();
    const onServerBack = vi.fn();
    startEphemeralServerWatchdog({
      checkHealth: async () => healthy,
      teardown,
      onServerBack,
    });

    await vi.advanceTimersByTimeAsync(10_000);
    expect(teardown).not.toHaveBeenCalled();

    healthy = false;
    await vi.advanceTimersByTimeAsync(2000 * 3);
    expect(teardown).toHaveBeenCalledTimes(1);
    expect(onServerBack).not.toHaveBeenCalled();

    healthy = true;
    await vi.advanceTimersByTimeAsync(3000);
    expect(onServerBack).toHaveBeenCalledTimes(1);
    expect(teardown).toHaveBeenCalledTimes(1);
  });

  it('a recovered probe resets the strike count', async () => {
    let healthy = true;
    const teardown = vi.fn();
    startEphemeralServerWatchdog({
      checkHealth: async () => healthy,
      teardown,
      onServerBack: vi.fn(),
    });
    healthy = false;
    await vi.advanceTimersByTimeAsync(4000); // two strikes
    healthy = true;
    await vi.advanceTimersByTimeAsync(2000);
    healthy = false;
    await vi.advanceTimersByTimeAsync(4000); // two strikes again
    expect(teardown).not.toHaveBeenCalled();
  });

  it('stop() halts polling', async () => {
    const checkHealth = vi.fn(async () => false);
    const teardown = vi.fn();
    const stop = startEphemeralServerWatchdog({ checkHealth, teardown, onServerBack: vi.fn() });
    stop();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(checkHealth).not.toHaveBeenCalled();
    expect(teardown).not.toHaveBeenCalled();
  });
});
