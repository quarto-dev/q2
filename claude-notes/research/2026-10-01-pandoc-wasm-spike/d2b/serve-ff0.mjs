// Task 0 by hand: `node build-typst.mjs && node serve-ff0.mjs`, open the URL in Firefox, wait for DONE; the JSON lands in results/h10a-0a-firefox.json.
import path from "node:path";
import { fileURLToPath } from "node:url";
process.env.REPORT = path.join(path.dirname(fileURLToPath(import.meta.url)), "results/h10a-0a-firefox.json");
const { listen } = await import("./serve.mjs");
const { port } = await listen(8141);
console.log(`serving on http://localhost:${port}/ff0.html  (report -> ${process.env.REPORT})`);
