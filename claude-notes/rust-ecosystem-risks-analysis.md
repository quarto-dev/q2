# Rust Ecosystem Risks Analysis

**Date**: 2025-10-12
**Research Focus**: Long-term risks for Rust projects regarding external dependencies, ecosystem stability, edition migrations, and security vulnerability communication

## Executive Summary

Rust offers excellent technical foundations (memory safety, strong type system) but faces **typical open-source sustainability challenges**, amplified by:
- Young ecosystem (compared to npm, Maven, PyPI)
- Culture of many small, composable crates (increases dependency counts)
- Single-maintainer dominance (62% of popular crates)

**Key finding**: Technical infrastructure is strong, but **human/process factors** pose the primary long-term risks.

---

## 1. Dependency & Ecosystem Risks

### Critical Issues

**1.1 Crate Abandonment & Maintainer Fatigue**
- **No guarantee** of long-term maintenance for any crate
- **62%** of popular crates have only **one maintainer**
- **44%** of crates don't receive regular security updates
- Average project pulls in **40+ indirect dependencies**
- Maintainers experience burnout, job changes, loss of interest

**Quote from community**: "The ecosystem of crates in Rust is very fragile. At any moment, an important crate can be abandoned by its creator, and block all the projects on which they depend."

**1.2 Supply Chain Security Vulnerabilities**
- **Attack vectors**:
  - Typo-squatting (e.g., "rustdecimal" vs "rust_decimal")
  - Maintainer account hijacking
  - Malicious `build.rs` scripts (can run arbitrary code at build time)
  - Malicious procedural macros
- Deep dependency nesting makes auditing difficult
- 200+ new crates published weekly

**1.3 API Instability**
- Crates below v1.0 can make breaking changes in minor versions
- No enforceable stability guarantees even for mature crates
- Dependency API changes can cascade through chains

**1.4 Dependency Graph Complexity**
- Each dependency is fundamentally a **liability**
- Transitive dependencies create large, hard-to-audit graphs
- Optional features increase compile times and complexity

### Mitigating Factors

**Positive Developments (2025)**
- Rust Foundation: Trusted Publishing launched on crates.io
- Crate signing infrastructure using TUF (The Update Framework) in progress
- Ecosystem growing at 2.1× per year in downloads

**Available Security Tools**
- `cargo-audit` - RustSec vulnerability scanning
- `cargo-supply-chain` - Maintainer/publisher data analysis
- `cargo-deny` - Security, licensing, and policy issues
- `cargo-outdated` - Tracks stale dependencies
- Resources like `blessed.rs` for finding recommended crates

**Best Practices**
- Be extremely conservative about adding dependencies
- Evaluate: popularity, active maintenance, security record, license
- Regularly update dependencies (but don't pin to specific versions)
- Prefer stable (≥1.0) crates from well-established maintainers
- Consider vendoring or forking critical dependencies

### Risk Assessment

**Overall Risk Level**: **Medium to High**

Trade-offs:
- ✅ Rust's memory safety reduces one class of long-term maintenance burden
- ⚠️ Dependency selection becomes critical architectural decision
- ⚠️ May need to vendor or fork critical dependencies for long-term stability

---

## 2. Rust Editions: Migration & Ecosystem Impact

### Core Design Principles

**The Key Guarantee**: Crates in one edition **seamlessly interoperate** with those compiled with other editions.

How it works:
- Each crate declares edition independently in `Cargo.toml`
- All editions compile to **same internal representation**
- Rust 2015 ↔ Rust 2024 crates can depend on each other
- Edition choice is "private" - doesn't affect up/downstream deps

**Design Constraint**: This compatibility limits editions to "skin deep" changes:
- New keywords (e.g., `async`, `await`, `gen`)
- Syntax improvements (e.g., module system, match ergonomics)
- Lint defaults
- Cannot alter semantics or runtime behavior

### Historical Track Record

#### Rust 2018 (First major edition)
**Changes**: Module system overhaul, async/await keywords, path clarity

**Community Experience**:
- ✅ Generally smooth transition
- ✅ `cargo fix` automated most changes successfully
- ✅ Module system "significantly for the better"
- ⚠️ Pain points: Macro imports still clunky, dependency renaming had bugs
- Migration sequence: enable preview → `cargo fix` → update Cargo.toml

**Ecosystem Impact**: Minimal fragmentation, "extremely smooth" transition

#### Rust 2021
**Changes**: Closure captures, panic macros consistency, new prelude items

**Experience**:
- ✅ Even smoother than 2018
- ✅ Most projects migrated with minimal issues
- ✅ `cargo fix` handled majority automatically

#### Rust 2024 (Released Feb 2025)
**Changes**: RPIT capture rules, `gen` keyword, match ergonomics, `unsafe` on certain stdlib functions

**Real-world Experience** (large codebase: ~400 crates, 1,500+ deps):
- ✅ Successfully migrated
- **Recommended strategy**: Incremental, not "big bang"
  1. Update code generation tools first
  2. Enable compatibility lints gradually
  3. Fix issues compatible with both editions
  4. Then flip edition flag
- **Challenges**:
  - `bindgen` and `cxx` needed updates
  - `std::env::set_var` now requires `unsafe`
  - Some `cargo fix` suggestions reduced readability
  - Doctests not auto-migrated
  - Macro fragment specifiers needed manual attention

### Ecosystem Consequences

**✅ Minimal Fragmentation**
- No "edition hell" (unlike Python 2 vs 3)
- Dependencies don't need to coordinate migrations
- Gradual transitions over months/years are fine

**~ Adoption Lag**
- Proc-macros and build-time codegen can lag
- Popular libraries migrate within months
- Unmaintained crates may stay on older editions indefinitely
- **This is mostly fine** due to compatibility guarantees

**~ Migration Automation Quality**
- `cargo fix` handles 80-90% of typical cases
- Corner cases require manual intervention:
  - Doctests always manual
  - Complex macro usage
  - Generated code
  - Build scripts
- Some automated fixes technically correct but stylistically poor

**+ Testing Burden**
- Library maintainers must test against new editions
- Potential for subtle behavior changes
- Increased CI/testing matrix complexity

**- Knowledge Fragmentation (Minor)**
- Documentation/examples may be edition-specific
- Learning resources need to indicate edition
- Stack Overflow answers may be edition-dependent

### Long-term Strategic View

**For Rust Project**: Editions are a **net positive** innovation:
- Allow language evolution without ecosystem breakage
- Provide clear migration checkpoints
- Avoid Python 2→3 or Perl 5→6 disasters

**Risk Level**: **Low to Medium**
- Track record excellent (3 successful editions)
- Design fundamentally prevents fragmentation
- Rust Foundation committed to stability

**Practical Implications for Kyoto**:
1. Don't worry about edition choice today - can always migrate later
2. Mixed edition dependencies work fine
3. Budget time for migrations - every 3 years expect days/weeks of work
4. Unmaintained dependencies stuck on old editions are less concerning than those that break with new compiler versions

**Verdict**: Edition system is one of Rust's **success stories** for managing ecosystem evolution. **Not a major risk factor** compared to dependency abandonment or supply chain security.

---

## 3. Security & Vulnerability Communication

### Core Infrastructure

#### RustSec Advisory Database (Central Hub)
- **What**: Community-maintained vulnerability DB for crates.io
- **Maintained by**: Rust Secure Code Working Group
- **Format**: Markdown + TOML front matter
- **Repository**: github.com/rustsec/advisory-db
- **Identifiers**: RUSTSEC-YYYY-NNNN

**Reporting Pathways**:
1. Direct PR to advisory-db (using template)
2. Open issue on advisory-db repo
3. Email: rustsec@googlegroups.com

**Turnaround**: Advisories reviewed and assigned IDs before publication

### Developer-Facing Tools

**1. cargo-audit** (Primary Tool)
- **Basic**: `cargo audit` checks Cargo.lock against RustSec
- **Auto-fix**: `cargo audit fix` updates Cargo.toml (experimental)
- **Binary scanning**: `cargo audit bin <binary>`
  - Works best with `cargo-auditable` (embeds full dependency tree)
  - Can partially recover deps from panic messages
  - Enables scanning production binaries

**2. cargo-deny**
- Advanced auditing: security + licensing + policy
- Checks transitive dependencies

**3. Ecosystem Tool Integration**
- **Trivy** v0.31.0+: Scans binaries/containers
- **Grype** v0.83.0+: Scans binaries/container images
- **osv-scanner** v2.0.1+: Reads embedded dependency data

### Ecosystem Integration

**GitHub Integration**
- GitHub Advisory Database imports all RustSec advisories in real-time
- Available via GitHub's public API
- Enables **Dependabot**:
  - Auto-detect vulnerable dependencies
  - Create PRs with security updates
  - Alert repository owners

**Open Source Vulnerabilities (OSV)**
- RustSec exports all data to OSV in real-time
- Google's open-source vulnerability DB format
- Enables interoperability with broader security tooling

### Rust Language/Stdlib Vulnerabilities

**Reporting**:
- Email: security@rust-lang.org
- 24-hour acknowledgment, 48-hour detailed response

**5-Step Disclosure Process**:
1. Assign primary handler
2. Confirm problem, identify affected versions, involve experts
3. Audit for similar issues
4. Prepare fixes, reserve CVE
5. Hold fixes privately until coordinated disclosure

**Notification**:
- **Public**: rustlang-security-announcements mailing list (low traffic)
- **Early warning**: distros@lists.openwall.com (3-day advance for medium+ severity)
- **Blog posts**: blog.rust-lang.org for major vulnerabilities

### Strengths

✓ **Well-structured central database** (RustSec)
✓ **Easy-to-use tooling** (cargo-audit standard practice)
✓ **Good ecosystem integration** (GitHub, OSV, Dependabot)
✓ **Binary scanning capability** (unique advantage)
✓ **Open and collaborative** (PRs welcome, CC BY 4.0 licensed)
✓ **Real-time exports** to other vulnerability databases

### Significant Gaps & Challenges

**1. Slow Disclosure Timeline** ⚠️
- Research: **Average 2+ years** from discovery to public disclosure
- **One-third of vulnerabilities** have no fix before disclosure
- **Major risk factor**

**2. No Dedicated CVE Numbering Authority** ⚠️
- Rust Foundation not yet a CNA
- Must coordinate with other CNAs for CVE assignment
- Slows formal vulnerability tracking
- **Recommendation**: Rust Foundation should apply for CNA status

**3. Single-Maintainer Vulnerability** ⚠️
- Many security-critical crates maintained by one person
- If unresponsive, vulnerability fixes can stall
- No formal process for security maintenance takeover

**4. Limited Proactive Notification** ⚠️
- Developers must actively run `cargo audit` or enable Dependabot
- No push notification system for critical vulnerabilities
- Easy to miss advisories without regular checking

**5. Incomplete Coverage**
- Not all vulnerabilities get RustSec advisories
- Some maintainers fix vulnerabilities quietly
- Research found 433 vulnerabilities; unclear how many in RustSec

**6. Binary Analysis Immaturity**
- Reverse engineering tools (Ghidra, etc.) struggle with Rust binaries
- Harder to audit compiled code
- Limits third-party security research

### Comparison to Other Ecosystems

| Feature | Rust | JavaScript | Python | Go |
|---------|------|------------|--------|-----|
| Central advisory DB | ✓ | ✓ | ✓ | ✓ |
| Auto-scanning tool | ✓ (cargo-audit) | ✓ (npm audit) | ✓ (pip-audit) | ✓ (govulncheck) |
| Binary scanning | ✓✓ (unique) | ✗ | ✗ | ✓ |
| GitHub integration | ✓ | ✓ | ✓ | ✓ |
| Dedicated CNA | ✗ | ✓ | ✓ | ✓ |
| Disclosure speed | ⚠️ (slow) | ~ | ~ | ~ |

### Practical Recommendations for Kyoto

#### Must Do
1. **Run `cargo audit` in CI/CD pipeline** - catches vulnerabilities early
2. **Enable Dependabot** on GitHub repos - automates security updates
3. **Subscribe to rustlang-security-announcements** - low-volume, high-signal
4. **Use `cargo-auditable`** in production builds - enables binary scanning

#### Should Consider
5. **Use `cargo-deny`** for stricter dependency policy enforcement
6. **Monitor RustSec advisories** periodically: rustsec.org/advisories
7. **Document security update process** - especially for deps with known CVEs
8. **Plan for abandoned dependencies** - who forks? who maintains?

#### Nice to Have
9. **Automated alerts** - GitHub Actions for daily/weekly cargo audit runs
10. **SBOM generation** - track dependencies for compliance/audit

### Bottom Line

**Communication infrastructure**: **Good** for developer-facing tools and ecosystem integration

**Actual vulnerability management**: **Concerning** due to:
- Slow disclosure timelines (2+ years average)
- Lack of CNA status
- Single-maintainer risk
- Passive notification model

**Risk Profile**: Infrastructure exists and works reasonably well, but **human/process factors** are weak points. Can't rely solely on ecosystem for timely vulnerability info - proactive scanning/monitoring essential.

For Kyoto/Quarto long-term project:
- Security tooling integration should be part of CI from day one
- Dependency selection should factor in maintainer responsiveness
- Plan for scenario where critical dependency has CVE with no fix

**Verdict**: Rust ecosystem is **ahead** in **tooling quality**, **behind** in **process maturity** around vulnerability disclosure and coordination.

---

## Overall Risk Assessment for Kyoto Project

### High-Level Risks (Prioritized)

1. **Dependency Abandonment** - Medium-High Risk
   - Mitigation: Conservative dependency selection, vendor critical deps

2. **Supply Chain Security** - Medium Risk
   - Mitigation: cargo-audit in CI, Dependabot, regular audits

3. **Slow Security Disclosure** - Medium Risk
   - Mitigation: Proactive scanning, don't rely on passive notifications

4. **API Instability** - Low-Medium Risk
   - Mitigation: Prefer v1.0+ crates, test updates before deploying

5. **Edition Migrations** - Low Risk
   - Mitigation: Budget days/weeks every 3 years, generally smooth

### Strategic Recommendations

**Dependency Philosophy**:
- Treat every dependency as a **liability requiring ongoing management**
- Prefer established crates with: multiple maintainers, v1.0+, active repos
- Consider forking/vendoring critical dependencies
- Document why each dependency is necessary

**Security Posture**:
- Integrate `cargo-audit` + `cargo-deny` in CI from day one
- Enable Dependabot on all repos
- Subscribe to rustlang-security-announcements
- Plan for "CVE with no fix" scenarios

**Edition Strategy**:
- Start with Edition 2024 (already decided)
- Mixed-edition dependencies are fine
- Budget time for future migrations but don't over-plan

**Ecosystem Maturity**:
- Rust ecosystem is young but rapidly maturing
- Infrastructure quality is excellent
- Process/governance is catching up
- Bet on continued improvement over next 3-5 years

### Comparison to TypeScript/Node.js (Quarto CLI's current stack)

Rust is **better** on:
- Memory safety (eliminates whole class of vulnerabilities)
- Binary scanning capabilities (cargo-auditable unique)
- Compilation catches more bugs earlier

Rust is **similar** on:
- Supply chain security challenges
- Single-maintainer risks
- Security tooling quality

Rust is **worse** on:
- Ecosystem maturity (younger)
- Security disclosure speed (2+ years average)
- Total number of available libraries

**Net assessment**: Rust's technical advantages (memory safety, type system) outweigh ecosystem maturity concerns for a long-term project like Kyoto, **provided** dependency selection is careful and security practices are proactive.

---

## References

- RustSec Advisory Database: https://rustsec.org/
- Rust Security Policy: https://www.rust-lang.org/policies/security
- Rust Edition Guide: https://doc.rust-lang.org/edition-guide/
- "A Closer Look at the Security Risks in the Rust Ecosystem" (ACM 2023)
- Rust Foundation Technology Report 2025
- Community discussions on crate sustainability (users.rust-lang.org)

---

## Session Context

**Date**: 2025-10-12
**Research Questions Answered**:
1. What are long-term risks for Rust projects in terms of external dependencies and ecosystem?
2. What are the ecosystem consequences of migrating from one edition of Rust to another?
3. How are security/vulnerability reports communicated across the ecosystem?

**Next Steps**: Consider implications for Kyoto dependency selection strategy and CI/CD security tooling setup.
