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
QUARTO_JOBS=1 QUARTO_PERF_STATS=1 cargo run -q --bin q2 -- render .
#   perf.sass hits=2 compiles=1 uncached=0 stale=0
grep -o 'quarto-theme-[a-f0-9]*\.css' _site/index.html | head -1       # 1dea42e453982763
sed -i '' 's/#123456/#abcdef/' _colors.scss
QUARTO_JOBS=1 QUARTO_PERF_STATS=1 cargo run -q --bin q2 -- render .
#   fixed: perf.sass hits=2 compiles=1 uncached=0 stale=1   (stale = manifest miss)
#   bug:   perf.sass hits=3 compiles=0 uncached=0           (served the old CSS)
grep -o 'quarto-theme-[a-f0-9]*\.css' _site/index.html | head -1       # fixed: 6b75e5a4dec17318
ls .quarto/cache/sass | grep -v -E '_lru_index|_version' | wc -l      # 1 (same key, overwritten)
head -2 .quarto/cache/sass/*[!x]                                       # envelope header + manifest
git checkout -- _colors.scss
```

Results and interpretation: `../2026-09-14-scss-cache-import-closure.md`
("Repro at HEAD"). `_site/` and `.quarto/` are gitignored.
