// Serve the harness on a fixed port for a by-hand browser; the page posts its results to /report (REPORT=path).
import { listen } from "./serve.mjs";
const { port } = await listen(8141);
console.log(`serving on http://localhost:${port}/ff.html`);
