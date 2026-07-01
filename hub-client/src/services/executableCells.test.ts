import { describe, it, expect } from 'vitest';
import { hasExecutableCells } from './executableCells';

describe('hasExecutableCells', () => {
  it('detects braced engine cells', () => {
    expect(hasExecutableCells('```{r}\n1 + 1\n```')).toBe(true);
    expect(hasExecutableCells('text\n\n```{python}\nprint(1)\n```\n')).toBe(true);
    expect(hasExecutableCells('```{ojs}\nx = 1\n```')).toBe(true);
  });

  it('accepts cell options after the language', () => {
    expect(hasExecutableCells('```{r echo=false}\n1\n```')).toBe(true);
  });

  it('accepts tilde fences and up to 3 spaces of indent', () => {
    expect(hasExecutableCells('~~~{r}\n1\n~~~')).toBe(true);
    expect(hasExecutableCells('   ```{python}\n1\n```')).toBe(true);
  });

  it('ignores the dotted display-class form (not executable)', () => {
    expect(hasExecutableCells('```{.python}\nprint(1)\n```')).toBe(false);
    expect(hasExecutableCells('```{.r .numberLines}\n1\n```')).toBe(false);
  });

  it('ignores plain fences and prose', () => {
    expect(hasExecutableCells('```\nplain code\n```')).toBe(false);
    expect(hasExecutableCells('```python\nprint(1)\n```')).toBe(false); // language, no braces
    expect(hasExecutableCells('# A heading\n\nSome prose with `inline` code.')).toBe(false);
    expect(hasExecutableCells('')).toBe(false);
  });
});
