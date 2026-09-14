# bd-m3hga05o investigation artifacts

`fixture/` is a copy of the bd-79c4do6g repro (branch
`braid/bd-79c4do6g-scss-cache-key-path`, room-1 checkout): a website with
`theme: [theme.scss]`, where `theme.scss` does `@import "_colors"`, and one
document each at depth 0, 1 and 2. Here it is used for the *other* bug that
repro surfaced: editing the imported partial does not invalidate the sass
cache.

```bash
cd fixture
rm -rf .quarto/cache/sass _site
cargo run -q --bin q2 -- render .
grep -o '\.repro{[^}]*}' _site/site_libs/quarto/quarto-theme-*.css     # #123456
sed -i '' 's/#123456/#abcdef/' _colors.scss
cargo run -q --bin q2 -- render .
grep -o '\.repro{[^}]*}' _site/site_libs/quarto/quarto-theme-*.css     # still #123456 at HEAD
git checkout -- _colors.scss
```

Results and interpretation: `../2026-09-14-scss-cache-import-closure.md`
("Repro at HEAD"). `_site/` and `.quarto/` are gitignored.
