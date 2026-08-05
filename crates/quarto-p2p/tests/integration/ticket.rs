//! `PreviewShareTicket` encode/decode/redaction tests (plan Phase 1).

use iroh::{EndpointAddr, SecretKey, TransportAddr};
use iroh_tickets::endpoint::EndpointTicket;
use quarto_p2p::{PreviewShareTicket, TicketParseError};

fn sample_addr() -> EndpointAddr {
    let id = SecretKey::from_bytes(&[7u8; 32]).public();
    EndpointAddr::from_parts(
        id,
        [
            TransportAddr::Relay("https://relay.example.com./".parse().unwrap()),
            TransportAddr::Ip("127.0.0.1:4433".parse().unwrap()),
            TransportAddr::Ip("[::1]:4433".parse().unwrap()),
        ],
    )
}

fn sample_token() -> [u8; 32] {
    core::array::from_fn(|i| [0xde, 0xad, 0xbe, 0xef][i % 4])
}

#[test]
fn roundtrip() {
    let ticket = PreviewShareTicket {
        addr: sample_addr(),
        token: sample_token(),
    };

    let s = ticket.to_string();
    assert!(
        s.starts_with("q2preview"),
        "join string must start with the q2preview KIND, got: {s}"
    );

    let parsed: PreviewShareTicket = s.parse().expect("roundtrip parse");
    assert_eq!(parsed, ticket);
}

#[test]
fn rejects_garbage_and_foreign_kinds() {
    // Empty string: no KIND prefix.
    let err = "".parse::<PreviewShareTicket>().unwrap_err();
    assert!(
        matches!(err, TicketParseError::Kind { .. }),
        "empty string should fail on the KIND prefix, got: {err:?}"
    );

    // Random text: no KIND prefix either.
    let err = "hello world".parse::<PreviewShareTicket>().unwrap_err();
    assert!(
        matches!(err, TicketParseError::Kind { .. }),
        "non-ticket text should fail on the KIND prefix, got: {err:?}"
    );

    // Correct prefix but not base32.
    let err = "q2preview!!!not-base32!!!"
        .parse::<PreviewShareTicket>()
        .unwrap_err();
    assert!(
        matches!(err, TicketParseError::Encoding { .. }),
        "invalid base32 should fail decoding, got: {err:?}"
    );

    // Correct prefix, valid base32 ("zzzzzzzz" decodes to 0xff bytes),
    // but garbage postcard payload.
    let err = "q2previewzzzzzzzz"
        .parse::<PreviewShareTicket>()
        .unwrap_err();
    assert!(
        matches!(err, TicketParseError::Postcard { .. }),
        "garbage payload should fail postcard decoding, got: {err:?}"
    );

    // A bare iroh EndpointTicket (KIND "endpoint") is a foreign kind.
    let foreign = EndpointTicket::new(sample_addr()).to_string();
    assert!(foreign.starts_with("endpoint"));
    let err = foreign.parse::<PreviewShareTicket>().unwrap_err();
    assert!(
        matches!(err, TicketParseError::Kind { .. }),
        "foreign ticket kinds must be rejected, got: {err:?}"
    );
}

#[test]
fn debug_redacts_token() {
    let ticket = PreviewShareTicket {
        addr: sample_addr(),
        token: sample_token(),
    };

    let dbg = format!("{ticket:?}");

    // Not as hex…
    assert!(
        !dbg.to_lowercase().contains("deadbeef"),
        "Debug leaked the token as hex: {dbg}"
    );
    // …not as a derived byte-array dump (0xde, 0xad = 222, 173)…
    assert!(
        !dbg.contains("222, 173"),
        "Debug leaked the token as a byte array: {dbg}"
    );
    // …and not by embedding the full join string (base32 encodes the token).
    assert!(
        !dbg.contains(&ticket.to_string()),
        "Debug embedded the full join string: {dbg}"
    );

    assert!(
        dbg.contains("redacted"),
        "Debug should mark the token as redacted: {dbg}"
    );
}
