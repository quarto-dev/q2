//! The router-wide `Host` gate (preview VFS contract D6).
//!
//! The preview server binds loopback, but binding keeps out other
//! machines, not other web pages: a page that rebinds its DNS name to
//! `127.0.0.1` is same-origin with the preview and could read and sync the
//! project. Such a request carries the attacker's name in `Host`, so the
//! gate refuses any request whose `Host` is not one of:
//!
//! - an IP literal (`127.0.0.1`, `[::1]`, a LAN address when bound wide):
//!   a rebinding page always sends a DNS name;
//! - `localhost` or a `*.localhost` name;
//! - the `--host` value the server was started with;
//! - a name listed in `QUARTO_PREVIEW_ALLOWED_HOSTS` (comma-separated), the
//!   escape for a reverse proxy that presents its own name (Posit
//!   Workbench, a container port-forward). `*` turns the gate off.
//!
//! A request with no `Host` at all cannot come from a browser and passes.
//! `--join` guests pass because their browser talks to the guest's own
//! loopback port.

use std::net::IpAddr;
use std::sync::OnceLock;

use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The environment variable naming extra allowed hosts.
pub const ALLOWED_HOSTS_ENV: &str = "QUARTO_PREVIEW_ALLOWED_HOSTS";

static GATE: OnceLock<HostGate> = OnceLock::new();

/// Which `Host` names the preview server answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostGate {
    /// Lower-cased names allowed beyond the built-in ones.
    names: Vec<String>,
    /// `*` was listed: allow every host.
    any: bool,
}

impl HostGate {
    /// The gate for a server bound to `bind_host`, widened by the value of
    /// [`ALLOWED_HOSTS_ENV`] when set.
    pub fn new(bind_host: &str, allowed_env: Option<&str>) -> Self {
        let mut names = vec![normalize(bind_host)];
        let mut any = false;
        for entry in allowed_env.unwrap_or("").split(',') {
            let entry = entry.trim();
            if entry == "*" {
                any = true;
            } else if !entry.is_empty() {
                names.push(normalize(host_name(entry)));
            }
        }
        Self { names, any }
    }

    /// Whether a request whose `Host` header is `host` may be answered.
    pub fn allows(&self, host: &str) -> bool {
        if self.any {
            return true;
        }
        if host.trim().parse::<IpAddr>().is_ok() {
            return true;
        }
        let name = normalize(host_name(host));
        if name.parse::<IpAddr>().is_ok() || name == "localhost" || name.ends_with(".localhost") {
            return true;
        }
        self.names.contains(&name)
    }
}

/// Install the gate for this server. First call wins (the server runs once
/// per process).
pub fn init(bind_host: &str) {
    let env = std::env::var(ALLOWED_HOSTS_ENV).ok();
    let _ = GATE.set(HostGate::new(bind_host, env.as_deref()));
}

/// The installed gate, or the loopback-only default when [`init`] was not
/// called (router tests that build the router directly).
fn gate() -> &'static HostGate {
    GATE.get_or_init(|| HostGate::new("127.0.0.1", None))
}

/// Middleware: refuse a request whose `Host` the gate does not allow.
pub async fn require_allowed_host(request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| request.uri().host().map(str::to_owned));
    match host {
        Some(host) if !gate().allows(&host) => {
            tracing::warn!(%host, "Refused a request for a host this preview does not serve");
            (
                StatusCode::FORBIDDEN,
                format!(
                    "This preview server does not answer for host {host:?}. If you reach \
                     it through a proxy, add the name to {ALLOWED_HOSTS_ENV}."
                ),
            )
                .into_response()
        }
        _ => next.run(request).await,
    }
}

/// The host name of a `Host` value: the port and IPv6 brackets removed.
fn host_name(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    }
}

fn normalize(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_names_and_ip_literals_pass() {
        let gate = HostGate::new("127.0.0.1", None);
        for host in [
            "127.0.0.1",
            "127.0.0.1:4100",
            "[::1]:4100",
            "::1",
            "localhost",
            "LOCALHOST:4100",
            "localhost.",
            "preview.localhost:4100",
            "192.168.1.20:4100",
        ] {
            assert!(gate.allows(host), "{host}");
        }
    }

    #[test]
    fn foreign_names_are_refused() {
        let gate = HostGate::new("127.0.0.1", None);
        for host in [
            "evil.example",
            "evil.example:4100",
            "localhost.evil.example",
            "127.0.0.1.evil.example",
            "",
        ] {
            assert!(!gate.allows(host), "{host}");
        }
    }

    #[test]
    fn the_bind_host_and_the_env_list_are_allowed() {
        let gate = HostGate::new(
            "devbox.lan",
            Some(" workbench.example.com , proxy.internal:8443,"),
        );
        assert!(gate.allows("devbox.lan:4100"));
        assert!(gate.allows("Workbench.Example.com"));
        assert!(gate.allows("proxy.internal"));
        assert!(!gate.allows("evil.example"));
    }

    #[test]
    fn a_star_turns_the_gate_off() {
        let gate = HostGate::new("127.0.0.1", Some("*"));
        assert!(gate.allows("anything.example"));
    }
}
