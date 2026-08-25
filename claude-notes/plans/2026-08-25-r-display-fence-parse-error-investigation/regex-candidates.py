import re, pathlib
CUR = re.compile(r"`r\s+([^`]+)`")                 # q2 today
KNITR = re.compile(r"(?<!(^``))(?<!(\n``))`r[ #]([^`]+)\s*`", re.M)  # knitr upstream
Q1LIKE = re.compile(r"(^|[^`])`r[ #]([^`]+)`", re.M)                # proposed port

R = pathlib.Path("/Users/gordon/src/q2-positron-docs/llms-info/repros/knitr-inline-r-eats-fence")
# Simulate STAGE 1: qmd writer collapses "``` r" and "```{.r}" to "```r"
def stage1(s):
    s = re.sub(r"^``` r$", "```r", s, flags=re.M)
    s = re.sub(r"^```\{\.r\}$", "```r", s, flags=re.M)
    return s

for fx in ["repro","nospace","attr-fence","yaml-title","control","workaround"]:
    src = (R/fx/"index.qmd").read_text()
    s = stage1(src)
    def show(rx,name):
        ms = list(rx.finditer(s))
        if not ms: return f"{name}=none"
        return f"{name}=" + "; ".join(repr(m.group(0)[:40]) for m in ms)
    print(f"--- {fx}")
    print("   ", show(CUR,"current"))
    print("   ", show(KNITR,"knitr"))
    print("   ", show(Q1LIKE,"proposed"))

print("\n--- inline-R sanity (must still match) ---")
for probe in ["The answer is `r 1+1`.", "`r x` at start", "a `r  x + 1  `.", "text `r#c` hash",
              "```r\nbody\n```", "````r\nbody with ` tick\n````", 'title: "```r blocks"',
              "Backtick prefix ``r x`"]:
    print(repr(probe))
    for rx,name in ((CUR,"current"),(KNITR,"knitr"),(Q1LIKE,"proposed")):
        ms=[m.group(0)[:40] for m in rx.finditer(probe)]
        print(f"     {name:9}: {ms}")
