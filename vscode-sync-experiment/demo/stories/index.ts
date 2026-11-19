import type { Demo } from '../src/demo';
import type { Step } from '../src/driver';
import { hubStory } from './hub';
import { hubCloneStory } from './hub-clone';

export function storyFor(d: Demo): Step[] {
  switch (d.o.preset) {
    case 'hub-browser':
    case 'hub-vscode-host':
      return hubStory(d);
    case 'hub-vscode-clone':
      return hubCloneStory(d);
    default:
      throw new Error(`preset ${d.o.preset} has no scripted story yet`);
  }
}
