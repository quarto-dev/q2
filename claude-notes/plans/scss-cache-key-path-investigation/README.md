# bd-79c4do6g investigation artifacts

`fixture/` is the smallest project that shows the sass cache key varying
per document directory: a website with `theme: [theme.scss]` (which
`@import`s `_colors.scss`) and one document each at depth 0, 1 and 2.

```bash
cd fixture
rm -rf .quarto/cache/sass _site
cargo run -q --bin q2 -- render .
ls .quarto/cache/sass | grep -v -E '_lru_index|_version' | wc -l   # 3 at HEAD; 1 once fixed
```

Results and interpretation: `../2026-09-13-scss-cache-key-path.md`
("Repro at HEAD"). `_site/` and `.quarto/` are gitignored.
