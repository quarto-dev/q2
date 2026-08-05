//! Single integration-test binary for quarto-p2p (see
//! `.claude/rules/integration-tests.md`).
//!
//! All tests are hermetic: `presets::Minimal`, `RelayMode::Disabled`,
//! explicit loopback `TransportAddr::Ip` addrs — no n0 infrastructure in CI.

pub mod support;
pub mod ticket;
pub mod tunnel;
