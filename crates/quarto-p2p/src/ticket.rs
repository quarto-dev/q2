//! The `q2preview…` join string: host `EndpointAddr` + session token.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use iroh::{EndpointAddr, EndpointId, TransportAddr};
use iroh_tickets::{ParseError, Ticket};
use serde::{Deserialize, Serialize};

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

/// Wire format, following the `iroh-tickets` versioned-enum convention:
/// the postcard enum tag doubles as the body version, alongside the KIND
/// string that tags the protocol itself.
#[derive(Serialize, Deserialize)]
enum TicketWireFormat {
    Variant1(Variant1PreviewShareTicket),
}

#[derive(Serialize, Deserialize)]
struct Variant1PreviewShareTicket {
    id: EndpointId,
    addrs: BTreeSet<TransportAddr>,
    token: [u8; TOKEN_LEN],
}

impl PreviewShareTicket {
    /// Whether the host published a relay address into this ticket.
    ///
    /// `false` means the host's endpoint never came online with a relay
    /// (or runs a hermetic test preset): guests can join over direct/LAN
    /// paths only. Exposed so consumers can report reachability without
    /// matching on iroh's `TransportAddr` themselves.
    pub fn has_relay_addr(&self) -> bool {
        self.addr
            .addrs
            .iter()
            .any(|a| matches!(a, TransportAddr::Relay(_)))
    }
}

impl Ticket for PreviewShareTicket {
    const KIND: &'static str = "q2preview";

    fn encode_bytes(&self) -> Vec<u8> {
        let data = TicketWireFormat::Variant1(Variant1PreviewShareTicket {
            id: self.addr.id,
            addrs: self.addr.addrs.clone(),
            token: self.token,
        });
        postcard::to_stdvec(&data).expect("postcard serialization failed")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        let data: TicketWireFormat = postcard::from_bytes(bytes)?;
        let TicketWireFormat::Variant1(Variant1PreviewShareTicket { id, addrs, token }) = data;
        Ok(Self {
            addr: EndpointAddr { id, addrs },
            token,
        })
    }
}

impl fmt::Display for PreviewShareTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encode_string())
    }
}

impl FromStr for PreviewShareTicket {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::decode_string(s)
    }
}

impl fmt::Debug for PreviewShareTicket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewShareTicket")
            .field("addr", &self.addr)
            .field("token", &"[redacted]")
            .finish()
    }
}
