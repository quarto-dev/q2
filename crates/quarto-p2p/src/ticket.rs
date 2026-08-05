//! The `q2preview…` join string: host `EndpointAddr` + session token.

use std::fmt;
use std::str::FromStr;

use iroh::EndpointAddr;
use iroh_tickets::{ParseError, Ticket};

use crate::TOKEN_LEN;

/// Join-string payload: the host's endpoint address plus the session token.
///
/// Possession of the string is the capability: the token authenticates the
/// guest to the host (per-stream prefix), while QUIC's handshake against the
/// pinned `EndpointId` authenticates the host to the guest.
///
/// `Debug` redacts the token; `Display` prints the full join string (which
/// necessarily encodes the token — that is what a join string is for).
#[derive(Clone, PartialEq, Eq)]
pub struct PreviewShareTicket {
    pub addr: EndpointAddr,
    pub token: [u8; TOKEN_LEN],
}

impl Ticket for PreviewShareTicket {
    const KIND: &'static str = "q2preview";

    fn encode_bytes(&self) -> Vec<u8> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }

    fn decode_bytes(_bytes: &[u8]) -> Result<Self, ParseError> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}

impl fmt::Display for PreviewShareTicket {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}

impl FromStr for PreviewShareTicket {
    type Err = ParseError;

    fn from_str(_s: &str) -> Result<Self, Self::Err> {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}

impl fmt::Debug for PreviewShareTicket {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("Phase 1 (bd-v8mwzpmi)")
    }
}
