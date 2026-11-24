/** The text of the journal (DM9): every page but `index.qmd` is a paper. Data are simulated with fixed seeds. */

export type Variant = 'main' | 'main-edit' | 'revision' | 'feature';

/** What the authors type in the demo: on-topic paragraphs, long enough to be seen across the room. */
export const EDITS = {
  /** Maurice's paragraph, typed under the Introduction of the fusion spectra paper. */
  maurice:
    'Spectral analysis is a natural fit for fusion events: if anastomosis is driven by the humidity cycle, the periodogram of fusion times should concentrate its power at the driving frequency.',
  mauriceNeedle: 'concentrate its power at the driving frequency',
  /** Maurice's unsaved idea, typed and then discarded by Myra's `git reset --hard`. */
  idea: 'An open question for the discussion: does the spectral peak survive when colonies of different ages are pooled into a single series?',
  ideaNeedle: 'colonies of different ages are pooled',
  /** Maurice's lost thought, retyped after the rebase in different words. */
  ideaAgain: 'One open question for the discussion: would the spectral peak hold up if we pooled colonies of different ages into one series?',
  ideaAgainNeedle: 'pooled colonies of different ages into one series',
  /** Myra's reply, typed under the Introduction at the end of the clone story to show the two are connected again. */
  myra: 'The humidity chamber runs on a 24-hour schedule, so the periodogram should peak near one cycle per day; a peak elsewhere would be worth a second look.',
  myraNeedle: 'peak near one cycle per day',
  /** Maurice's answer to Myra's reply, typed in his clone: the two have converged when it reaches her. */
  mauriceReply:
    'Power at two cycles per day would not be a defect: it would suggest that fusion responds separately to the wetting and drying limbs of the cycle.',
  mauriceReplyNeedle: 'wetting and drying limbs of the cycle',
  /** Myra's answer to Maurice's retyped question about pooling, at the end of the hub story. */
  myraReply:
    'Pooling is defensible only if the colonies share a phase, so we shall test for phase coherence across colonies before combining the series.',
  myraReplyNeedle: 'test for phase coherence across colonies',
  /** The same, for the tip extension paper in the vscode-share story. */
  tip: 'Tip extension depends on turgor, and turgor follows the humidity schedule with a lag, so we expect the dry and cycling regimes to differ most at the start of each cycle.',
  tipNeedle: 'dry and cycling regimes to differ most',
  /** Myra's copy edit: both Introduction paragraphs open with "Tip extension", so she rewords her own opening. */
  copyLine: 'remains poorly characterized',
  copyLead: 'Growth at the hyphal tip',
  copyNeedle: 'Growth at the hyphal tip remains poorly characterized',
} as const;

export const GITIGNORE = '.venv/\n__pycache__/\n.quarto/\n*_files/\n_site/\n';

export function quartoYml(): string {
  return `project:
  type: website

quarto-required: ">=2.0.0"

website:
  title: "Journal of Theoretical Mycodynamics"

preview:
  engine: auto

format:
  html:
    theme: cosmo
`;
}

const AFFIL = 'Institute for Applied Mycodynamics';

interface Meta {
  title: string;
  authors: string[];
  doi: string;
  abstract: string;
  keywords: string[];
}

function front(m: Meta): string {
  const authors = m.authors.map((a) => `  - name: "${a}"\n    affiliation: "${AFFIL}"`).join('\n');
  return `---
title: "${m.title}"
author:
${authors}
doi: "${m.doi}"
keywords: [${m.keywords.join(', ')}]
abstract: "${m.abstract}"
---
`;
}

const REFS = `## References

1. Rott, B., & Mildew, C. (2025). Preliminary evidence for fusion in dry substrates. *J. Theor. Mycodyn.* **16**(4), 211-219.
2. Celia, M. (2025). On the calibration of humidity chambers. *J. Theor. Mycodyn.* **16**(2), 33-48.
3. Glass, N. L., & Fleissner, A. (2006). Re-wiring the network. *Fungal Genet. Biol.* **43**, 1-9.
`;

function prose(topic: string): string {
  return `## Introduction

${topic} remains poorly characterized in the heterokaryotic setting. We follow the formalism of Glass and Fleissner [3]
and extend it to oscillating substrate humidity.

## Methods

Colonies were grown on defined medium at 22 °C under a sinusoidal humidity schedule. All simulations use fixed seeds.

## Discussion

The effect is small, robust, and almost certainly real.
`;
}

const R_CHUNK_MAIN = `\`\`\`{r}
#| echo: true
#| fig-width: 6
#| fig-height: 3.2
library(ggplot2)
set.seed(17)
regime <- rep(c("dry", "cycling", "wet"), each = 40)
mu <- c(dry = 1.2, cycling = 1.9, wet = 2.4)[regime]
d <- data.frame(regime = factor(regime, c("dry", "cycling", "wet")),
                rate = rlnorm(120, log(mu), 0.25))
fit <- lm(log(rate) ~ regime, d)
ggplot(d, aes(rate, regime, fill = regime)) +
  geom_boxplot(alpha = 0.7, show.legend = FALSE) +
  scale_x_log10() +
  labs(x = "tip extension rate (um/min, log scale)", y = NULL)
\`\`\`
`;

const R_CHUNK_FEATURE = R_CHUNK_MAIN.replace('log(rate) ~ regime', 'log(rate) ~ regime + I(seq_along(rate) %% 7)').replace(
  'alpha = 0.7',
  'alpha = 0.7, notch = TRUE',
);

const PY_CHUNK_MAIN = `\`\`\`{python}
#| echo: true
import numpy as np
import pandas as pd
from plotnine import ggplot, aes, geom_line, labs, theme_minimal

rng = np.random.default_rng(23)
t = np.arange(512)
signal = np.sin(2 * np.pi * t / 32) + 0.6 * rng.standard_normal(512)
power = np.abs(np.fft.rfft(signal)) ** 2 / 512
freq = np.fft.rfftfreq(512)
df = pd.DataFrame({"frequency": freq[1:], "power": power[1:]})
(ggplot(df, aes("frequency", "power")) + geom_line(color="#2a7a4b")
 + labs(x="frequency (cycles per step)", y="periodogram power") + theme_minimal())
\`\`\`
`;

const PY_CHUNK_FEATURE = PY_CHUNK_MAIN.replace('0.6 *', '0.9 *');

const ABSTRACT: Record<Variant, string> = {
  main: 'Hyphal tip extension differs across three humidity regimes (p = 0.049).',
  'main-edit': 'Hyphal tip extension differs reliably across three humidity regimes (p = 0.049).',
  revision: 'Hyphal tip extension appears to differ across humidity regimes, though marginally (p = 0.051).',
  feature: 'After reanalysis with a corrected variance structure, tip extension differs across regimes (p = 0.031).',
};

const PY_ABSTRACT: Record<Variant, string> = {
  main: 'The periodogram of fusion events shows a peak at the humidity oscillation frequency.',
  'main-edit': 'The periodogram of fusion events shows a reliable peak at the humidity oscillation frequency.',
  revision: 'The periodogram of fusion events appears to show a peak at the humidity oscillation frequency, though marginally.',
  feature: 'After reanalysis with a stronger noise model, the periodogram of fusion events shows a sharper peak at the humidity oscillation frequency.',
};

export function papers(v: Variant): Record<string, string> {
  const feat = v === 'feature';
  const out: Record<string, string> = {
    'celia-morel-2026-anastomosis-kinetics.qmd':
      front({
        title: 'Stochastic Anastomosis Kinetics in Heterokaryotic Mycelial Networks Under Oscillating Substrate Humidity',
        authors: ['Myra Celia', 'Maurice Morel'],
        doi: '10.5555/jtm.17.3.001',
        abstract: 'We characterize hyphal fusion as a stochastic process in heterokaryotic networks under oscillating humidity.',
        keywords: ['anastomosis', 'heterokaryon', 'humidity'],
      }) +
      `\n![Network micrograph. Scale bar 10 µm.](figures/micrograph.png)\n\n` +
      prose('Anastomosis kinetics') +
      REFS,
    'celia-2026-humidity-chamber-calibration.qmd':
      front({
        title: 'Calibration of a Programmable Humidity Chamber for Mycodynamic Assays',
        authors: ['Myra Celia'],
        doi: '10.5555/jtm.17.3.002',
        abstract: 'A programmable chamber holds relative humidity within 0.4 percentage points over 72 hours.',
        keywords: ['calibration', 'instrumentation'],
      }) +
      `\n| Reagent | Concentration | Supplier |\n|---|---|---|\n| Glucose | 20 g/L | Spore & Co. |\n| Asparagine | 2 g/L | Spore & Co. |\n| Agar | 15 g/L | Hypha Supply |\n\n` +
      prose('Chamber calibration') +
      REFS,
    'celia-2026-tip-extension-lmm.qmd':
      front({
        title: 'Hyphal Tip Extension Under Three Humidity Regimes: A Mixed-Effects Analysis',
        authors: ['Myra Celia'],
        doi: '10.5555/jtm.17.3.003',
        abstract: ABSTRACT[v],
        keywords: ['tip extension', 'mixed-effects model', 'variance components'],
      }) +
      `\n${prose('Tip extension')}\n## Results\n\n${feat ? R_CHUNK_FEATURE : R_CHUNK_MAIN}\n` +
      REFS,
    'morel-2026-fusion-spectra.qmd':
      front({
        title: 'Fusion Events as a Point Process: Spectral Signatures of Anastomosis',
        authors: ['Maurice Morel'],
        doi: '10.5555/jtm.17.3.004',
        abstract: PY_ABSTRACT[v],
        keywords: ['point process', 'periodogram', 'bootstrap'],
      }) +
      `\n${prose('Fusion spectra')}\n## Results\n\n${feat ? PY_CHUNK_FEATURE : PY_CHUNK_MAIN}\n` +
      REFS,
  };
  if (!feat) {
    out['rott-mildew-2025-preliminary-fusion.qmd'] =
      front({
        title: 'Preliminary Evidence for Hyphal Fusion in Dry Substrates',
        authors: ['Bartholomew Rott', 'Clementine Mildew'],
        doi: '10.5555/jtm.16.4.011',
        abstract: 'We report fusion events in substrates previously thought too dry to support them.',
        keywords: ['pilot study', 'dry substrates'],
      }) +
      prose('Fusion in dry substrates') +
      REFS;
  } else {
    out['celia-2026-corrigendum-variance-components.qmd'] =
      front({
        title: 'Corrigendum: Variance Components in the Tip Extension Analysis',
        authors: ['Myra Celia'],
        doi: '10.5555/jtm.17.3.005',
        abstract: 'We correct the variance structure of the earlier mixed-effects analysis and retract the pilot study that relied on it.',
        keywords: ['corrigendum', 'variance components'],
      }) +
      `\nThe pilot study of Rott and Mildew [1] has been retracted.\n\n` +
      REFS;
  }
  return out;
}

export function indexQmd(files: string[]): string {
  const rows = files
    .filter((f) => f.endsWith('.qmd') && f !== 'index.qmd')
    .sort()
    .map((f) => `- [${f.replace(/\.qmd$/, '')}](${f})`)
    .join('\n');
  return `---
title: "Journal of Theoretical Mycodynamics"
subtitle: "Volume 17, Issue 3"
---

## Table of contents

${rows}
`;
}
