// bd-nbv80e33 sanity check — NOT a committed test (browser-level tests are
// bd-rlxlrja3). Loads the rendered repro page in jsdom with its scripts and
// exercises pagination, category filtering and the no-matching reveal.
//
//   cd <repo> && (cd claude-notes/plans/listing-listjs-init-investigation/repro && q2 render)
//   node claude-notes/plans/listing-listjs-init-investigation/jsdom-check.cjs \
//     "$PWD" "$PWD/claude-notes/plans/listing-listjs-init-investigation/repro/_site"
//
// Result at commit time (2026-10-09): listings ["listing-listing"], page1
// [Post 1, Post 2], page links [1, 2], page2 [Post 3], cat3 [Post 3],
// no-matching hidden until a filter empties the list, no script errors.
const path = require("path");
const fs = require("fs");
const { JSDOM, VirtualConsole, ResourceLoader } = require(path.join(process.argv[2], "node_modules/jsdom"));
class SiteLoader extends ResourceLoader {
  fetch(url) {
    const u = new URL(url);
    const file = path.join(site, decodeURIComponent(u.pathname));
    return fs.existsSync(file) ? Promise.resolve(fs.readFileSync(file)) : Promise.resolve(Buffer.from(""));
  }
}
const site = process.argv[3];
const vc = new VirtualConsole();
const errors = [];
vc.on("jsdomError", (e) => errors.push(String(e.message || e)));
vc.on("error", (e) => errors.push(String(e)));
Promise.resolve(new JSDOM(fs.readFileSync(path.join(site, "index.html"), "utf8"), {
  runScripts: "dangerously", resources: new SiteLoader(), url: "http://localhost/index.html", virtualConsole: vc,
})).then((dom) => {
  const w = dom.window, d = w.document;
  w.addEventListener("load", () => {
    setTimeout(() => {
      const titles = () => [...d.querySelectorAll("#listing-listing .list .listing-title")].map((e) => e.textContent);
      const out = {};
      out.listings = Object.keys(w["quarto-listings"] || {});
      out.page1 = titles();
      out.pageLinks = [...d.querySelectorAll("#listing-listing .pagination .page")].map((e) => e.textContent);
      const p2 = [...d.querySelectorAll("#listing-listing .pagination .page")].find((e) => e.textContent === "2");
      if (p2) p2.click();
      out.page2 = titles();
      w.quartoListingCategory(Buffer.from(encodeURIComponent("cat3")).toString("base64"));
      out.cat3 = titles();
      out.noMatchingHidden = d.querySelector("#listing-listing .listing-no-matching").classList.contains("d-none");
      w["quarto-listings"]["listing-listing"].filter(() => false);
      out.noMatchingHiddenWhenEmpty = d.querySelector("#listing-listing .listing-no-matching").classList.contains("d-none");
      out.errors = errors;
      console.log(JSON.stringify(out, null, 1));
      w.close();
    }, 200);
  });
});
