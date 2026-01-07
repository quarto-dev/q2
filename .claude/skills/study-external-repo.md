---
name: study-external-repo
description: Studies external GitHub repositories by cloning them locally for efficient searching. Use when needing to analyze, understand, or search code from external GitHub repositories instead of using slow web fetching. Triggers proactively when GitHub repository URLs appear in conversation.
---

# Study External Repository

Clone external GitHub repositories locally for efficient code exploration instead of using slow, limited web fetching.

## When to Use This Skill

**Proactively activate when you see:**
- GitHub repository URLs (e.g., `https://github.com/org/repo`)
- References to external libraries or projects you need to study
- Questions requiring code search across an external codebase
- Need to understand implementation details in another project

**Do NOT use web fetching for GitHub code exploration.** Local clones are:
- Much faster for repeated access
- Searchable with Glob/Grep
- Complete (all files, not just what you navigate to)

## Directory Structure

All external repositories go in: `/work/projects/repos/github/quarto-dev/kyoto/external-sources/`

Use **nested org/repo naming**: `external-sources/{org}/{repo}`

Examples:
- `https://github.com/jgm/pandoc` → `external-sources/jgm/pandoc`
- `https://github.com/nickel-lang/nickel` → `external-sources/nickel-lang/nickel`

## Step-by-Step Process

### 1. Extract org and repo from URL

Parse the GitHub URL to get organization and repository name:
```
https://github.com/{org}/{repo}[.git]
```

### 2. Check if already cloned

```bash
ls -la /work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}/{repo} 2>/dev/null
```

If the directory exists, skip to step 4.

### 3. Clone the repository

**For most repositories (shallow clone recommended):**
```bash
mkdir -p /work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}
git clone --depth 1 https://github.com/{org}/{repo}.git /work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}/{repo}
```

**For repositories where you need full history** (e.g., to study commit evolution):
```bash
mkdir -p /work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}
git clone https://github.com/{org}/{repo}.git /work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}/{repo}
```

**Shallow clone tradeoffs:**
- ✅ Much faster download
- ✅ Less disk space
- ❌ No git history/blame
- ❌ Cannot checkout other branches without fetching

Use shallow clone by default unless history is specifically needed.

### 4. Search and explore locally

Now use standard local tools instead of WebFetch:

```bash
# Find files by pattern
Glob: pattern="**/*.rs" path="/work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}/{repo}"

# Search for code patterns
Grep: pattern="function_name" path="/work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}/{repo}"

# Read specific files
Read: file_path="/work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}/{repo}/src/main.rs"
```

## Special Cases

### Specific branch or tag needed

```bash
# Clone then checkout
git clone --depth 1 --branch {branch_or_tag} https://github.com/{org}/{repo}.git /work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}/{repo}
```

### Update an existing shallow clone

```bash
cd /work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}/{repo}
git fetch --depth 1
git reset --hard origin/HEAD
```

### Need to see a file at a specific commit

For shallow clones, you may need to deepen:
```bash
cd /work/projects/repos/github/quarto-dev/kyoto/external-sources/{org}/{repo}
git fetch --depth=100  # or more as needed
git show {commit}:{filepath}
```

## Important Reminders

1. **Never commit changes** to external-sources repos - they are read-only references
2. **Prefer shallow clones** unless you specifically need git history
3. **Use the Task tool with Explore agent** for complex codebase exploration
4. **Check existing clones first** before cloning again

## Currently Cloned Repositories

To see what's already available:
```bash
find /work/projects/repos/github/quarto-dev/kyoto/external-sources -maxdepth 2 -type d | head -20
```
