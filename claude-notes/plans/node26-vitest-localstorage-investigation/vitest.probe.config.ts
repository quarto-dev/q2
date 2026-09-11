// Probe config: hub-client's unit-test config + the prototype shim as a setup file.
// Run from hub-client/:  npx vitest run --config ../claude-notes/plans/node26-vitest-localstorage-investigation/vitest.probe.config.ts <files>
import path from 'path';
import { mergeConfig } from 'vitest/config';
import base from '../../../hub-client/vitest.config';
const hubClient = path.resolve(__dirname, '../../../hub-client');
export default mergeConfig(base, {
  test: {
    root: hubClient,
    setupFiles: [path.resolve(__dirname, 'webstorage-shim.ts')],
  },
});
