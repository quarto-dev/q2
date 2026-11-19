import { Demo, defaultOptions, type DemoOptions } from '../../src/demo';
import { playStory } from '../../src/cli';

export const live: Demo[] = [];

/** A demo with its terminal window open and the observer running; the hub is the caller's to start. */
export async function startDemo(over: Partial<DemoOptions>): Promise<Demo> {
  const d = await Demo.create(defaultOptions({ headless: true, pollMs: 400, ...over }));
  live.push(d);
  await d.openHostTerminal();
  d.startObserver();
  return d;
}

/** Free play: the launcher starts the hub and opens both editors. */
export async function startFreePlay(over: Partial<DemoOptions> = {}): Promise<Demo> {
  const d = await startDemo(over);
  if (d.o.preset === 'hubless') {
    await d.startQhubProcess();
    await d.hublessAll();
  } else {
    await d.startHubProcess();
    await d.openEditors();
  }
  return d;
}

export async function stopAll(): Promise<void> {
  for (const d of live.splice(0)) {
    await d.stop().catch(() => {});
    d.removeRoot();
  }
}

export { playStory };
