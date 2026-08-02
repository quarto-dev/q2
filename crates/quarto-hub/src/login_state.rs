//! Sealed, short-lived login state (H2, `bd-uqjiac5a`).
//!
//! A **sealed login-state blob** is an HMAC-signed, expiring token the
//! hub hands to a browser in a cookie before sending it to the IdP, and
//! verifies when the IdP's response comes back. It lets the hub bind an
//! ID token to *the login attempt it started* without holding server-side
//! state.
//!
//! Today it carries a `nonce` for the GIS flow: without it, any captured
//! Google ID token can be replayed to a mint endpoint for its full (~1 h)
//! validity, because signature/`iss`/`aud`/`exp` all still check out.
//!
//! # Domain separation
//!
//! The blob is signed with the **same keys** as session tokens
//! ([`crate::session::SessionKeys`]), so it must be impossible to
//! confuse the two — a sealed login blob accepted as a session cookie
//! would be an authentication bypass. Two independent barriers:
//!
//! 1. **Distinct `iss`.** [`LOGIN_STATE_ISSUER`] vs
//!    [`crate::session::SESSION_ISSUER`], and both verifiers pin theirs.
//! 2. **Required `typ`.** [`LOGIN_STATE_TYP`] must be present and exact;
//!    session tokens have no `typ` claim at all.
//!
//! Beyond those, the claim sets are disjoint: `SessionClaims` requires
//! `sub`/`email`/`sid`/`auth_time`, which a login blob does not carry, so
//! deserialization alone already fails. Tests assert both directions.
//!
//! # Reuse
//!
//! Pattern (ii)'s `OidcState` callback (Epic 2's B1, `bd-qxgoti2b`) needs
//! exactly this shape — mint / verify / expiry / domain separation — for
//! its PKCE `state`. Add a variant here rather than re-deriving it
//! inline; see `claude-notes/plans/2026-07-27-auth-current-flow-hardening.md`.

use serde::{Deserialize, Serialize};

use crate::session::SessionKeys;

/// `iss` on every sealed login-state blob.
///
/// Deliberately **not** [`crate::session::SESSION_ISSUER`]: the session
/// verifier pins that value, so a login blob can never pass as a session
/// token even if the claim sets ever converged.
pub const LOGIN_STATE_ISSUER: &str = "quarto-hub-login";

/// Required `typ` on every sealed login-state blob. Versioned so a
/// future payload change can be rejected explicitly rather than
/// silently misread.
pub const LOGIN_STATE_TYP: &str = "login-nonce-v1";

/// How long a sealed login-state blob stays valid.
///
/// Bounds how long a captured blob is useful, and is the practical
/// ceiling on how long a user may sit on the IdP's consent screen before
/// their login must be restarted.
pub const LOGIN_STATE_TTL_SECS: i64 = 600;

/// Clock-skew leeway on the blob's `exp`, matching
/// [`crate::session::SESSION_LEEWAY_SECS`].
pub const LOGIN_STATE_LEEWAY_SECS: i64 = 60;

/// Claims payload of a sealed login-state blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginStateClaims {
    /// Always [`LOGIN_STATE_ISSUER`].
    pub iss: String,
    /// Always [`LOGIN_STATE_TYP`].
    pub typ: String,
    /// The nonce the IdP must echo back in the ID token.
    pub nonce: String,
    pub iat: i64,
    pub exp: i64,
}

/// Why a sealed login-state blob failed to open. Variants map 1:1 to the
/// distinguishable audit classes; none carries blob contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginStateError {
    /// JOSE header `kid` missing or not in the key map — signed under a
    /// different secret, or not a hub blob at all.
    UnknownKid,
    /// Signature, structure, `iss`, or `typ` invalid. Also the class a
    /// **session token** presented as a login blob lands in.
    Tampered,
    /// `exp` in the past, beyond leeway.
    Expired,
}

impl LoginStateError {
    /// Stable audit-log discriminator (`detail = "login_state_<class>"`).
    pub fn audit_class(&self) -> &'static str {
        match self {
            LoginStateError::UnknownKid => "kid_mismatch",
            LoginStateError::Tampered => "tampered",
            LoginStateError::Expired => "expired",
        }
    }
}

/// Generate a fresh login nonce: 32 random bytes, hex-encoded.
///
/// 256 bits because this value is what binds an ID token to one login
/// attempt — it must not be guessable by a party who can observe other
/// logins.
pub fn generate_login_nonce() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Seal a nonce into a signed, expiring blob valid from `now`.
pub fn seal_login_state(
    keys: &SessionKeys,
    nonce: &str,
    now: i64,
    ttl_secs: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let claims = LoginStateClaims {
        iss: LOGIN_STATE_ISSUER.to_string(),
        typ: LOGIN_STATE_TYP.to_string(),
        nonce: nonce.to_string(),
        iat: now,
        exp: now + ttl_secs,
    };
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    header.kid = Some(keys.current().kid().to_string());
    jsonwebtoken::encode(
        &header,
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(keys.current_secret()),
    )
}

/// Open a sealed login-state blob at time `now`.
///
/// Order: resolve `kid` (fail closed) → HS256 signature + structure +
/// `iss` under the resolved key → `typ` → `exp`. Verification accepts the
/// previous key during a rotation overlap, so a login started just before
/// a graceful rotation still completes.
pub fn open_login_state(
    keys: &SessionKeys,
    blob: &str,
    now: i64,
) -> Result<LoginStateClaims, LoginStateError> {
    let header = jsonwebtoken::decode_header(blob).map_err(|_| LoginStateError::Tampered)?;
    let kid = header.kid.as_deref().ok_or(LoginStateError::UnknownKid)?;
    let secret = keys
        .secret_for_kid(kid)
        .ok_or(LoginStateError::UnknownKid)?;

    // `exp` is checked below against the caller-supplied clock, so the
    // library's implicit system-clock check is disabled. The algorithm
    // set is pinned to HS256 — the header can never select another.
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.set_issuer(&[LOGIN_STATE_ISSUER]);
    validation.set_required_spec_claims(&["iss"]);

    let claims = jsonwebtoken::decode::<LoginStateClaims>(
        blob,
        &jsonwebtoken::DecodingKey::from_secret(secret),
        &validation,
    )
    .map_err(|_| LoginStateError::Tampered)?
    .claims;

    // Second domain barrier (see module docs). A session token cannot
    // reach here — it has no `typ` — but an explicit check keeps the
    // guarantee local rather than emergent.
    if claims.typ != LOGIN_STATE_TYP {
        return Err(LoginStateError::Tampered);
    }

    // A blob stamped from the future was never sealed by this hub.
    if claims.iat > now + LOGIN_STATE_LEEWAY_SECS {
        return Err(LoginStateError::Tampered);
    }

    if now > claims.exp + LOGIN_STATE_LEEWAY_SECS {
        return Err(LoginStateError::Expired);
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{SESSION_ISSUER, SessionIdentity, SessionLifetimes, mint_session};

    const SECRET_A: [u8; 32] = [0xA1; 32];
    const SECRET_B: [u8; 32] = [0xB2; 32];

    fn keys() -> SessionKeys {
        SessionKeys::new(SECRET_A)
    }

    fn now() -> i64 {
        1_800_000_000
    }

    #[test]
    fn seal_open_roundtrip() {
        let t = now();
        let blob = seal_login_state(&keys(), "abc123", t, LOGIN_STATE_TTL_SECS).unwrap();
        let opened = open_login_state(&keys(), &blob, t).unwrap();
        assert_eq!(opened.nonce, "abc123");
        assert_eq!(opened.iss, LOGIN_STATE_ISSUER);
        assert_eq!(opened.typ, LOGIN_STATE_TYP);
        assert_eq!(opened.iat, t);
        assert_eq!(opened.exp, t + LOGIN_STATE_TTL_SECS);
    }

    #[test]
    fn open_rejects_expired_blob() {
        let t = now();
        let blob = seal_login_state(&keys(), "abc123", t, LOGIN_STATE_TTL_SECS).unwrap();
        // Just inside the leeway still opens; past it does not.
        let edge = t + LOGIN_STATE_TTL_SECS + LOGIN_STATE_LEEWAY_SECS;
        assert!(open_login_state(&keys(), &blob, edge).is_ok());
        assert_eq!(
            open_login_state(&keys(), &blob, edge + 1),
            Err(LoginStateError::Expired)
        );
    }

    #[test]
    fn open_rejects_tampered_payload() {
        let t = now();
        let blob = seal_login_state(&keys(), "abc123", t, LOGIN_STATE_TTL_SECS).unwrap();
        let mut parts: Vec<String> = blob.split('.').map(String::from).collect();
        let mut payload = parts[1].clone().into_bytes();
        let i = payload.len() / 2;
        payload[i] = if payload[i] == b'A' { b'B' } else { b'A' };
        parts[1] = String::from_utf8(payload).unwrap();
        assert_eq!(
            open_login_state(&keys(), &parts.join("."), t),
            Err(LoginStateError::Tampered)
        );
    }

    #[test]
    fn open_rejects_blob_sealed_under_another_secret() {
        let t = now();
        let blob = seal_login_state(&SessionKeys::new(SECRET_B), "abc", t, 600).unwrap();
        // Different secret → different derived kid → fails closed at kid
        // resolution rather than as a signature error.
        assert_eq!(
            open_login_state(&keys(), &blob, t),
            Err(LoginStateError::UnknownKid)
        );
    }

    #[test]
    fn open_accepts_blob_sealed_under_the_previous_key() {
        // Graceful rotation: a login started just before the rotation
        // must still be completable.
        let t = now();
        let blob = seal_login_state(&SessionKeys::new(SECRET_B), "abc", t, 600).unwrap();
        let rotated = SessionKeys::with_previous(SECRET_A, SECRET_B);
        assert_eq!(open_login_state(&rotated, &blob, t).unwrap().nonce, "abc");
    }

    // ── domain separation (both directions) ───────────────────────

    #[test]
    fn a_session_token_is_not_a_valid_login_blob() {
        let t = now();
        let identity = SessionIdentity {
            sub: "sub-1".into(),
            email: "user@posit.co".into(),
            email_verified: true,
            name: None,
            picture: None,
        };
        let session = mint_session(&keys(), SessionLifetimes::default(), &identity, t).unwrap();
        assert_eq!(
            open_login_state(&keys(), &session, t),
            Err(LoginStateError::Tampered),
            "a session token must never open as login state"
        );
    }

    #[test]
    fn a_login_blob_is_not_a_valid_session_token() {
        // The mirror image, asserted here so both halves of the
        // separation live in one place. `verify_session` pins
        // `SESSION_ISSUER` and requires claims the blob lacks.
        let t = now();
        let blob = seal_login_state(&keys(), "abc", t, 600).unwrap();
        let err = crate::session::verify_session(&keys(), SessionLifetimes::default(), &blob, t)
            .expect_err("a login blob must never verify as a session");
        assert_eq!(err, crate::session::SessionVerifyError::Tampered);
    }

    #[test]
    fn issuers_are_distinct() {
        // The invariant the two tests above rest on.
        assert_ne!(LOGIN_STATE_ISSUER, SESSION_ISSUER);
    }

    #[test]
    fn generated_nonces_are_256_bit_and_unique() {
        let a = generate_login_nonce();
        let b = generate_login_nonce();
        assert_eq!(a.len(), 64, "32 bytes hex-encoded");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
