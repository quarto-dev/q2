// Drives one Playwright browser through the page. usage: node drive.mjs chromium|webkit [only]
import { listen } from "./serve.mjs";
import * as pw from "playwright";
const name = process.argv[2] || "chromium";
const { port, server } = await listen();
const b = await pw[name].launch();
const pg = await b.newPage();
pg.on("pageerror", (e) => console.log("PAGEERROR", e));
pg.on("console", (m) => console.log("console:", m.text()));
const only = process.argv[3] ? `&only=${process.argv[3]}` : "";
await pg.goto(`http://localhost:${port}/?b=${name}${only}`);
await pg.waitForFunction(() => document.getElementById("banner").textContent !== "running…", null, { timeout: 30 * 60 * 1000, polling: 500 });
console.log("banner:", await pg.textContent("#banner"));
await b.close(); server.close();
