// Drives the spike-3 page in a Playwright browser (BROWSER=chromium|webkit|firefox) and prints the banner + steps.
import * as pw from "playwright";
const b = await pw[process.env.BROWSER || "chromium"].launch(); const pg = await b.newPage();
await pg.goto(`http://localhost:${process.env.PORT || 8137}/`);
await pg.waitForFunction(() => /ALL PASSED|FAILED|WORKER ERROR/.test(document.getElementById("banner").textContent), null, { timeout: 120000 });
console.log(await pg.evaluate(() => document.getElementById("banner").textContent));
console.log(await pg.evaluate(() => [...document.querySelectorAll("#log li")].map(l => l.textContent).join("\n")));
await b.close();
