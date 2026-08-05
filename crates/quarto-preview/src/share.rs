//! `--share` glue (live-share plan Phase 2, bd-jhvkwosw): spawn the
//! quarto-p2p tunnel host in front of the preview server's loopback
//! port and announce the ready-to-paste join string.
//!
//! The tunnel is the *only* remote surface `--share` adds — the HTTP
//! port itself stays loopback-bound. Possession of the join string is
//! the capability, which is why the banner spells out exactly what it
//! grants (view + re-run, plus disk writes iff `--allow-edit`).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use quarto_p2p::{PreviewShareTicket, TunnelError, TunnelHost, TunnelHostConfig, TunnelHostHandle};

/// A live share session: the join ticket plus the running tunnel host.
pub struct ShareSession {
    /// The join ticket (host `EndpointAddr` + session token). Public so
    /// callers/tests can render or re-derive the join string.
    pub ticket: PreviewShareTicket,
    handle: TunnelHostHandle,
}

impl ShareSession {
    /// Gracefully shut the tunnel host down (closes the iroh endpoint).
    pub async fn shutdown(self) -> Result<(), TunnelError> {
        self.handle.shutdown().await
    }
}

/// Spawn the tunnel host targeting the preview server at
/// `share_target(host, port)` and announce the join banner via
/// `announce` (production passes a `println!` shim; tests capture).
///
/// The port must already be resolved — the CLI probes it before the
/// server starts, so the ticket exists (and prints) ahead of the first
/// accept; a too-fast guest just retries via its health supervisor.
///
/// With the production n0 preset this awaits the endpoint's relay
/// contact (bounded inside [`TunnelHost::spawn`], ~10 s worst case);
/// when no relay was reachable the minted ticket carries direct/LAN
/// addresses only, and the banner says so — quarto-p2p's own
/// `tracing::warn!` is invisible at the CLI's default `quarto=warn`
/// filter, so the banner is the user-facing signal.
pub async fn start_share_session(
    cfg: TunnelHostConfig,
    host: &str,
    port: u16,
    allow_edit: bool,
    announce: impl FnOnce(&str),
) -> Result<ShareSession, TunnelError> {
    let (ticket, handle) = TunnelHost::spawn(cfg, share_target(host, port)).await?;
    let banner = format_share_banner(&ticket.to_string(), allow_edit, ticket.has_relay_addr());
    announce(&banner);
    Ok(ShareSession { ticket, handle })
}

/// Render the share banner: capability warning + the bare
/// `q2 preview --join …` line (last, with nothing after it, so a
/// triple-click / drag copy survives terminal wrapping).
pub fn format_share_banner(join_string: &str, allow_edit: bool, relay_reachable: bool) -> String {
    let mut banner = String::new();
    banner.push_str("Sharing this preview session (end-to-end encrypted via iroh).\n");
    if allow_edit {
        banner.push_str(
            "Anyone with the join string below can VIEW the project, RE-RUN its code\n\
             on this machine, and EDIT the project's files on disk (--allow-edit):\n",
        );
    } else {
        banner.push_str(
            "Anyone with the join string below can VIEW the project and RE-RUN its\n\
             code on this machine:\n",
        );
    }
    if !relay_reachable {
        banner.push_str(
            "\nNote: no relay is reachable, so guests can join over direct/LAN\n\
             connections only.\n",
        );
    }
    banner.push('\n');
    banner.push_str(&format!("q2 preview --join {join_string}"));
    banner
}

/// Map the preview server's bind host to the tunnel's TCP target.
///
/// The default (and the plan's stated posture) is `127.0.0.1:{port}`.
/// The other arms keep `--share` working when `--host` was also set:
/// an unspecified bind (`0.0.0.0` / `::`) is reachable via the
/// matching loopback, a concrete IP is its own target, and a hostname
/// (e.g. `localhost`) falls back to IPv4 loopback.
fn share_target(host: &str, port: u16) -> SocketAddr {
    let ip = match host.parse::<IpAddr>() {
        Ok(ip) if ip.is_unspecified() => match ip {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
        },
        Ok(ip) => ip,
        Err(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
    };
    SocketAddr::new(ip, port)
}

#[cfg(test)]
mod tests {
    use super::share_target;

    #[test]
    fn share_target_maps_bind_hosts_to_dialable_targets() {
        for (host, expected) in [
            ("127.0.0.1", "127.0.0.1:7777"),
            ("0.0.0.0", "127.0.0.1:7777"),
            ("::", "[::1]:7777"),
            ("::1", "[::1]:7777"),
            ("192.168.1.5", "192.168.1.5:7777"),
            ("localhost", "127.0.0.1:7777"),
        ] {
            assert_eq!(
                share_target(host, 7777),
                expected.parse().unwrap(),
                "share_target({host:?})"
            );
        }
    }
}
