//! Assemble a `q2preview…` join string from the Gate-0 spike tunnel
//! host's printed parts (`TICKET <endpoint-ticket>` and `TOKEN <hex>`),
//! so the real `q2 preview --join` can join through a spike-hosted
//! tunnel leg — e.g. the throttled host-side splice used by the
//! live-share payload harness in `scripts/join-boot-baseline/`
//! (plan `claude-notes/plans/2026-08-13-live-share-local-spa-assets.md`,
//! Phase 4). The spike and the real tunnel share ALPN and the 32-byte
//! per-stream token prefix; only the ticket packaging differs.
//!
//! Usage:
//!   cargo run -p quarto-p2p --example preview-ticket-from-parts -- \
//!     <ENDPOINT_TICKET> <HEX_TOKEN>

use std::str::FromStr;

use iroh_tickets::endpoint::EndpointTicket;
use quarto_p2p::{PreviewShareTicket, TOKEN_LEN};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(ticket), Some(token_hex)) = (args.next(), args.next()) else {
        eprintln!("usage: preview-ticket-from-parts <ENDPOINT_TICKET> <HEX_TOKEN>");
        std::process::exit(2);
    };
    let addr = match EndpointTicket::from_str(&ticket) {
        Ok(t) => t.endpoint_addr().clone(),
        Err(e) => {
            eprintln!("invalid endpoint ticket: {e}");
            std::process::exit(1);
        }
    };
    let token = match hex_decode_token(&token_hex) {
        Ok(token) => token,
        Err(e) => {
            eprintln!("invalid token: {e}");
            std::process::exit(1);
        }
    };
    println!("{}", PreviewShareTicket { addr, token });
}

/// Decode the spike host's hex-printed session token.
fn hex_decode_token(s: &str) -> Result<[u8; TOKEN_LEN], String> {
    if s.len() != 2 * TOKEN_LEN {
        return Err(format!(
            "expected {} hex chars, got {}",
            2 * TOKEN_LEN,
            s.len()
        ));
    }
    let mut out = [0u8; TOKEN_LEN];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(out)
}
