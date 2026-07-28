//! Hub-minted session tokens with sliding expiry.
//!
//! # Token-format spec (epic `bd-ey6jg70f`)
//!
//! The hub validates a Google ID token **once** at login, then mints a
//! compact, hub-signed session token carried in the `quarto_hub_token`
//! HttpOnly cookie. Format: **HS256 JWT** signed with a dedicated
//! *session secret* (never the actor-id `server_secret` — different
//! blast radius).
//!
//! ## JOSE header
//!
//! Carries a **static `kid`** derived with domain separation:
//! first 8 hex chars of `HMAC-SHA256(session_secret, "quarto-hub-session-kid-v1")`.
//! Never a truncated plain hash of the secret (that would publish bits
//! of the secret's bare hash in every token). The `kid` buys
//! observability (verify failures distinguish "minted under a different
//! secret" from expired/tampered) and makes rotation a pure map
//! operation (C5b).
//!
//! ## Claims
//!
//! | claim            | semantics                                                       |
//! |------------------|-----------------------------------------------------------------|
//! | `iss`            | always `"quarto-hub"`; required by the verifier                  |
//! | `sub`            | Google `sub`, stamped at mint                                    |
//! | `email`, `email_verified`, `name`, `picture` | stamped from the Google claims validated at mint; consumed by `/auth/me` and the per-request allowlist re-check |
//! | `iat`            | time of the most recent (re-)issue                               |
//! | `auth_time`      | the original Google-validation instant; carried **unchanged** across every re-issue — the anchor for the absolute lifetime cap |
//! | `exp`            | sliding: `min(iat + idle, auth_time + absolute)`                 |
//! | `sid`            | random per login, carried unchanged across re-issues (identifies the session family; enables per-device revocation retrofit) |
//!
//! ## Lifetimes
//!
//! Sliding **idle timeout** (default 7 days) and **absolute max
//! lifetime** (default 30 days), anchored at `auth_time`. At every
//! (re-)issue: `exp = min(now + idle, auth_time + absolute)`.
//! Verification enforces `exp` **and, independently,**
//! `now < auth_time + absolute` — a re-issue bug can never extend a
//! session past the cap. Deployment-configurable via
//! `QUARTO_HUB_SESSION_IDLE_SECS` / `QUARTO_HUB_SESSION_ABSOLUTE_SECS`
//! (public deployments should prefer tighter caps).
//!
//! ## Verification
//!
//! Pinned algorithm set: `[HS256]` only, `DecodingKey::from_secret`.
//! The token header must never select the algorithm or an arbitrary
//! key: the `kid` is resolved by **exact-match lookup** in a
//! `kid → secret` map (size ≤ 2: current + optional previous during
//! rotation), **failing closed** on an unknown or missing `kid`.
//! Required claims: `iss` (= `"quarto-hub"`), `exp`. Leeway 60 s,
//! matching the Google path's `validate_azp_and_iat`.
//!
//! Verify failures are logged distinguishably (kid mismatch vs expired
//! vs absolute-cap vs tampered) and never include token contents.
//!
//! Plan: `claude-notes/plans/2026-07-06-hub-server-minted-sliding-sessions.md`.

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// `iss` value in every hub-minted session token; required by the verifier.
pub const SESSION_ISSUER: &str = "quarto-hub";

/// Domain-separation input for the static `kid` derivation. Versioned so a
/// future derivation change can coexist with the old one during rotation.
pub const SESSION_KID_DOMAIN: &str = "quarto-hub-session-kid-v1";

/// Default sliding idle timeout: 7 days.
pub const DEFAULT_SESSION_IDLE_SECS: i64 = 7 * 24 * 3600;

/// Default absolute max session lifetime, anchored at `auth_time`: 30 days.
pub const DEFAULT_SESSION_ABSOLUTE_SECS: i64 = 30 * 24 * 3600;

/// Clock-skew leeway for `exp` (and future-`iat`) checks, in seconds.
/// Matches the 60 s leeway used on the Google path (`validate_azp_and_iat`).
pub const SESSION_LEEWAY_SECS: i64 = 60;

/// Minimum token age before authenticated activity triggers a sliding
/// re-issue (§2c) — bounds `Set-Cookie` churn to ~1/hour per session.
/// A token signed under a non-current `kid` is re-issued regardless of
/// age, so sessions migrate promptly during graceful rotation.
pub const SESSION_REISSUE_MIN_AGE_SECS: i64 = 3600;

/// Claims payload of a hub-minted session token.
///
/// `name`/`picture` are optional (some Google accounts omit them); all
/// other claims are mandatory — a token missing any of them fails
/// verification as tampered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionClaims {
    /// Always [`SESSION_ISSUER`].
    pub iss: String,
    pub sub: String,
    pub email: String,
    pub email_verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    /// Time of the most recent (re-)issue (epoch seconds).
    pub iat: i64,
    /// Original Google-validation instant; immutable across re-issues.
    pub auth_time: i64,
    /// Sliding expiry: `min(iat + idle, auth_time + absolute)` at issue.
    pub exp: i64,
    /// Random per-login session-family id, immutable across re-issues.
    pub sid: String,
}

/// The identity stamped into a session token at mint, taken from the
/// Google claims validated at login.
#[derive(Debug, Clone)]
pub struct SessionIdentity {
    pub sub: String,
    pub email: String,
    pub email_verified: bool,
    pub name: Option<String>,
    pub picture: Option<String>,
}

/// Cap on the `name` claim carried into a session token (chars).
const IDENTITY_NAME_MAX_CHARS: usize = 200;

/// Cap on the `picture` URL carried into a session token (bytes);
/// longer values are dropped rather than truncated (a truncated URL is
/// useless).
const IDENTITY_PICTURE_MAX_BYTES: usize = 500;

impl SessionIdentity {
    /// Build the mint identity from validated Google claims.
    ///
    /// Defensively caps `name`/`picture`: identity claims are stamped
    /// into the cookie, so pathological IdP values must not push the
    /// session token toward the ~4096-byte browser cookie drop the
    /// compact format exists to avoid.
    pub fn from_oidc(claims: &crate::auth::OidcClaims) -> Self {
        let name = claims.name.clone().map(|n| {
            if n.chars().count() > IDENTITY_NAME_MAX_CHARS {
                n.chars().take(IDENTITY_NAME_MAX_CHARS).collect()
            } else {
                n
            }
        });
        let picture = claims
            .picture
            .clone()
            .filter(|p| p.len() <= IDENTITY_PICTURE_MAX_BYTES);
        Self {
            sub: claims.sub.clone(),
            email: claims.email.clone(),
            email_verified: claims.email_verified,
            name,
            picture,
        }
    }
}

/// Current unix time in seconds — the shared clock for production
/// mint/verify call sites (tests inject their own `now`).
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Sliding-session lifetime configuration (idle + absolute caps).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLifetimes {
    /// Sliding idle timeout in seconds.
    pub idle_secs: i64,
    /// Absolute max lifetime in seconds, anchored at `auth_time`.
    pub absolute_secs: i64,
}

impl Default for SessionLifetimes {
    fn default() -> Self {
        Self {
            idle_secs: DEFAULT_SESSION_IDLE_SECS,
            absolute_secs: DEFAULT_SESSION_ABSOLUTE_SECS,
        }
    }
}

impl SessionLifetimes {
    /// Resolve lifetimes from `QUARTO_HUB_SESSION_IDLE_SECS` /
    /// `QUARTO_HUB_SESSION_ABSOLUTE_SECS`, falling back to the defaults.
    /// Rejects non-positive or unparsable values, and an idle timeout
    /// longer than the absolute cap.
    pub fn from_env() -> Result<Self, String> {
        fn read_secs(var: &str, default: i64) -> Result<i64, String> {
            match std::env::var(var) {
                Ok(raw) => {
                    let secs: i64 = raw
                        .parse()
                        .map_err(|e| format!("{var}: invalid integer '{raw}': {e}"))?;
                    if secs <= 0 {
                        return Err(format!("{var}: must be positive, got {secs}"));
                    }
                    Ok(secs)
                }
                Err(_) => Ok(default),
            }
        }

        let idle_secs = read_secs("QUARTO_HUB_SESSION_IDLE_SECS", DEFAULT_SESSION_IDLE_SECS)?;
        let absolute_secs = read_secs(
            "QUARTO_HUB_SESSION_ABSOLUTE_SECS",
            DEFAULT_SESSION_ABSOLUTE_SECS,
        )?;
        if idle_secs > absolute_secs {
            return Err(format!(
                "QUARTO_HUB_SESSION_IDLE_SECS ({idle_secs}) must not exceed \
                 QUARTO_HUB_SESSION_ABSOLUTE_SECS ({absolute_secs})"
            ));
        }
        Ok(Self {
            idle_secs,
            absolute_secs,
        })
    }

    /// The expiry for a token (re-)issued at `now` for a session
    /// anchored at `auth_time`: `min(now + idle, auth_time + absolute)`.
    pub fn expiry(&self, now: i64, auth_time: i64) -> i64 {
        (now + self.idle_secs).min(auth_time + self.absolute_secs)
    }
}

/// A session-signing key: 32-byte secret + its derived static `kid`.
#[derive(Clone)]
pub struct SessionKey {
    kid: String,
    secret: [u8; 32],
}

impl SessionKey {
    pub fn new(secret: [u8; 32]) -> Self {
        Self {
            kid: derive_session_kid(&secret),
            secret,
        }
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }
}

impl std::fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never expose the secret in Debug output.
        f.debug_struct("SessionKey")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

/// Session key material: the current signing key plus, during a graceful
/// rotation window (C5b), the previous verification-only key.
///
/// Signing always uses `current`; verification resolves the token's
/// `kid` by exact match against `current` then `previous`, failing
/// closed on no match. The map never exceeds two entries.
#[derive(Debug, Clone)]
pub struct SessionKeys {
    current: SessionKey,
    previous: Option<SessionKey>,
}

impl SessionKeys {
    /// Single-secret configuration (no rotation overlap).
    pub fn new(secret: [u8; 32]) -> Self {
        Self {
            current: SessionKey::new(secret),
            previous: None,
        }
    }

    /// Rotation-overlap configuration (C5b): sign with `current`,
    /// verify against `current` + `previous`.
    pub fn with_previous(secret: [u8; 32], previous: [u8; 32]) -> Self {
        let current = SessionKey::new(secret);
        let previous = SessionKey::new(previous);
        if current.kid == previous.kid {
            // 8-hex kid collision (~1 in 2^32) — old-kid tokens would
            // resolve to the current key and fail as tampered instead
            // of verifying: fail-closed, but worth surfacing.
            tracing::warn!(
                kid = %current.kid,
                "current and previous session secrets derive the same kid; \
                 tokens signed under the previous secret will not verify"
            );
        }
        Self {
            current,
            previous: Some(previous),
        }
    }

    pub fn current(&self) -> &SessionKey {
        &self.current
    }

    pub fn previous(&self) -> Option<&SessionKey> {
        self.previous.as_ref()
    }

    /// Exact-match `kid` lookup (fail closed: `None` for unknown kids).
    fn key_for_kid(&self, kid: &str) -> Option<&SessionKey> {
        if kid == self.current.kid {
            return Some(&self.current);
        }
        self.previous.as_ref().filter(|p| p.kid == kid)
    }
}

/// Why a session token failed verification. The variants map 1:1 to the
/// distinguishable audit-log classes; none carries token contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionVerifyError {
    /// JOSE header `kid` missing or not in the key map — minted under a
    /// different secret (rotated away, config drift) or not a hub
    /// session token at all (e.g. a legacy Google-JWT cookie). Fails
    /// closed.
    UnknownKid,
    /// Signature/structure/issuer invalid under the resolved key.
    Tampered,
    /// `exp` in the past (beyond leeway).
    Expired,
    /// `now ≥ auth_time + absolute` — enforced independently of `exp`.
    AbsoluteCapExceeded,
}

impl SessionVerifyError {
    /// Stable audit-log discriminator (`detail = "session_<class>"`).
    pub fn audit_class(&self) -> &'static str {
        match self {
            SessionVerifyError::UnknownKid => "kid_mismatch",
            SessionVerifyError::Tampered => "tampered",
            SessionVerifyError::Expired => "expired",
            SessionVerifyError::AbsoluteCapExceeded => "absolute_cap",
        }
    }
}

/// A successfully verified session token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSession {
    pub claims: SessionClaims,
    /// Whether the token was signed under the *current* key. `false`
    /// during a rotation overlap window → re-issue promptly (§2c).
    pub signed_with_current_key: bool,
}

impl VerifiedSession {
    /// Whether authenticated activity at `now` should re-issue the
    /// cookie (§2c): token ≥ 1 h old, or signed under a non-current
    /// `kid`. `auth_time` immutability and the exp cap are enforced by
    /// [`reissue_session`] itself.
    pub fn should_reissue(&self, now: i64) -> bool {
        !self.signed_with_current_key || now - self.claims.iat >= SESSION_REISSUE_MIN_AGE_SECS
    }
}

/// Generate a fresh random session-family id (16 random bytes, hex).
pub fn generate_sid() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Derive the static `kid` for a session secret: first 8 hex chars of
/// `HMAC-SHA256(key = secret, message = SESSION_KID_DOMAIN)`.
///
/// Domain-separated HMAC, never a truncated plain hash — a bare
/// truncated `SHA-256(secret)` would publish bits of the secret's hash
/// in every token (cross-protocol-reuse hazard, and an offline
/// dictionary oracle for low-entropy operator-supplied secrets).
pub fn derive_session_kid(secret: &[u8; 32]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts keys of any length");
    mac.update(SESSION_KID_DOMAIN.as_bytes());
    let digest = mac.finalize().into_bytes();
    hex::encode(&digest[..4])
}

/// A freshly minted session: the signed token and the `sid` of the
/// session family it opens.
///
/// The `sid` is returned alongside rather than left inside the token so
/// the login path can name the new session in the audit log without
/// re-decoding what it just signed (H5).
#[derive(Debug, Clone)]
pub struct MintedSession {
    pub token: String,
    pub sid: String,
}

/// Mint a fresh session token at login: `auth_time = now`, fresh
/// random `sid`, identity stamped from the validated Google claims.
///
/// Returns the token only. The login path uses [`mint_session_at`],
/// which also surfaces the `sid`.
pub fn mint_session(
    keys: &SessionKeys,
    lifetimes: SessionLifetimes,
    identity: &SessionIdentity,
    now: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    Ok(mint_session_at(keys, lifetimes, identity, now, now)?.token)
}

/// [`mint_session`] with an explicit `auth_time` anchor.
///
/// Used by the login path when the user has a live revocation event:
/// `auth_time` is bumped to the revocation ledger's
/// [`crate::revocation::RevocationLedger::min_auth_time`] (at most one
/// second ahead of `now`), so a re-login in the same second as a
/// logout-everywhere is provably post-revocation instead of dying with
/// the revoked family.
pub fn mint_session_at(
    keys: &SessionKeys,
    lifetimes: SessionLifetimes,
    identity: &SessionIdentity,
    now: i64,
    auth_time: i64,
) -> Result<MintedSession, jsonwebtoken::errors::Error> {
    let sid = generate_sid();
    let claims = SessionClaims {
        iss: SESSION_ISSUER.to_string(),
        sub: identity.sub.clone(),
        email: identity.email.clone(),
        email_verified: identity.email_verified,
        name: identity.name.clone(),
        picture: identity.picture.clone(),
        iat: now,
        auth_time,
        exp: lifetimes.expiry(now, auth_time),
        sid: sid.clone(),
    };
    Ok(MintedSession {
        token: sign_claims(keys, &claims)?,
        sid,
    })
}

/// Re-issue a session token on authenticated activity: `iat = now`,
/// sliding `exp = min(now + idle, auth_time + absolute)`; `auth_time`,
/// `sid`, and the identity claims are carried over **unchanged**.
/// Always signs under the *current* key.
pub fn reissue_session(
    keys: &SessionKeys,
    lifetimes: SessionLifetimes,
    claims: &SessionClaims,
    now: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let mut reissued = claims.clone();
    reissued.iat = now;
    // auth_time and sid are deliberately carried over unchanged: the
    // absolute cap anchors to the original Google validation, and the
    // sid identifies the session family across re-issues.
    reissued.exp = lifetimes.expiry(now, claims.auth_time);
    sign_claims(keys, &reissued)
}

/// Sign an arbitrary claims payload under the current key, stamping the
/// key's `kid` into the JOSE header.
///
/// Building block for [`mint_session`]/[`reissue_session`]; public so
/// tests can construct tokens that violate the mint invariants (e.g. a
/// future `exp` past the absolute cap) and prove the verifier rejects
/// them independently.
pub fn sign_claims(
    keys: &SessionKeys,
    claims: &SessionClaims,
) -> Result<String, jsonwebtoken::errors::Error> {
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    header.kid = Some(keys.current.kid.clone());
    jsonwebtoken::encode(
        &header,
        claims,
        &jsonwebtoken::EncodingKey::from_secret(&keys.current.secret),
    )
}

/// Verify a session token at time `now`.
///
/// Order: resolve `kid` (fail closed) → HS256 signature + structure +
/// `iss` under the resolved key → `exp` (leeway
/// [`SESSION_LEEWAY_SECS`]) → absolute cap
/// (`now < auth_time + absolute`, independent of `exp`).
///
/// The allowlist re-check and revocation checks are the caller's
/// responsibility (they need `AuthConfig` / the revocation store).
pub fn verify_session(
    keys: &SessionKeys,
    lifetimes: SessionLifetimes,
    token: &str,
    now: i64,
) -> Result<VerifiedSession, SessionVerifyError> {
    // 1. Resolve the signing key from the JOSE header `kid` — exact
    //    match only, failing closed. A legacy Google-JWT cookie lands
    //    here too (its kid is a JWKS key id, never a hub session kid).
    let header = jsonwebtoken::decode_header(token).map_err(|_| SessionVerifyError::Tampered)?;
    let kid = header
        .kid
        .as_deref()
        .ok_or(SessionVerifyError::UnknownKid)?;
    let key = keys
        .key_for_kid(kid)
        .ok_or(SessionVerifyError::UnknownKid)?;

    // 2. Signature + structure + issuer under the resolved key. The
    //    algorithm set is pinned to HS256 — the token header can never
    //    select another algorithm or key type. `exp` is checked
    //    manually below against the caller-supplied clock, so the
    //    library's implicit system-clock check is disabled.
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.set_issuer(&[SESSION_ISSUER]);
    validation.set_required_spec_claims(&["iss"]);

    let token_data = jsonwebtoken::decode::<SessionClaims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(&key.secret),
        &validation,
    )
    .map_err(|_| SessionVerifyError::Tampered)?;
    let claims = token_data.claims;

    // A token stamped from the future is never something the hub
    // minted — treat as tampered (defense in depth vs clock games).
    if claims.iat > now + SESSION_LEEWAY_SECS || claims.auth_time > now + SESSION_LEEWAY_SECS {
        return Err(SessionVerifyError::Tampered);
    }

    // 3. Sliding expiry (with clock-skew leeway).
    if now > claims.exp + SESSION_LEEWAY_SECS {
        return Err(SessionVerifyError::Expired);
    }

    // 4. Absolute cap, independent of `exp`: a re-issue bug (or forged
    //    exp under a leaked signing path) can never extend a session
    //    past `auth_time + absolute`.
    if now >= claims.auth_time + lifetimes.absolute_secs {
        return Err(SessionVerifyError::AbsoluteCapExceeded);
    }

    let signed_with_current_key = kid == keys.current.kid;
    Ok(VerifiedSession {
        claims,
        signed_with_current_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET_A: [u8; 32] = [0xA1; 32];
    const SECRET_B: [u8; 32] = [0xB2; 32];

    fn identity() -> SessionIdentity {
        SessionIdentity {
            sub: "google-sub-123".to_string(),
            email: "user@posit.co".to_string(),
            email_verified: true,
            name: Some("Test User".to_string()),
            picture: Some("https://lh3.googleusercontent.com/p".to_string()),
        }
    }

    fn keys() -> SessionKeys {
        SessionKeys::new(SECRET_A)
    }

    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    // ── kid derivation (C1) ───────────────────────────────────────

    #[test]
    fn kid_is_deterministic_and_8_lowercase_hex() {
        let kid1 = derive_session_kid(&SECRET_A);
        let kid2 = derive_session_kid(&SECRET_A);
        assert_eq!(kid1, kid2, "kid must be deterministic");
        assert_eq!(kid1.len(), 8);
        assert!(
            kid1.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "kid must be lowercase hex: {kid1}"
        );
    }

    #[test]
    fn kid_differs_across_secrets() {
        assert_ne!(derive_session_kid(&SECRET_A), derive_session_kid(&SECRET_B));
    }

    #[test]
    fn kid_uses_domain_separated_hmac_not_plain_hash() {
        // The kid must be HMAC(secret, domain), NOT a truncated plain
        // SHA-256 of the secret (which would publish bits of the
        // secret's bare hash in every token).
        use sha2::Digest;
        let plain = hex::encode(sha2::Sha256::digest(SECRET_A));
        let kid = derive_session_kid(&SECRET_A);
        assert_ne!(kid, plain[..8], "kid must not be truncated SHA-256(secret)");

        // Pin the derivation: first 8 hex of HMAC-SHA256(secret, domain).
        let mut mac = HmacSha256::new_from_slice(&SECRET_A).unwrap();
        mac.update(SESSION_KID_DOMAIN.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());
        assert_eq!(kid, expected[..8]);
    }

    // ── lifetimes (C1/C2) ─────────────────────────────────────────

    #[test]
    fn default_lifetimes_are_7d_idle_30d_absolute() {
        let lt = SessionLifetimes::default();
        assert_eq!(lt.idle_secs, 7 * 24 * 3600);
        assert_eq!(lt.absolute_secs, 30 * 24 * 3600);
    }

    /// Serialize env-var tests (process-global state). nextest runs each
    /// test in its own process, but plain `cargo test` does not.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn lifetimes_from_env_defaults_when_unset() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // SAFETY: test-only env mutation, serialized by ENV_MUTEX.
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_IDLE_SECS") };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_ABSOLUTE_SECS") };
        assert_eq!(
            SessionLifetimes::from_env().unwrap(),
            SessionLifetimes::default()
        );
    }

    #[test]
    fn lifetimes_from_env_overrides() {
        let _guard = ENV_MUTEX.lock().unwrap();
        unsafe { std::env::set_var("QUARTO_HUB_SESSION_IDLE_SECS", "3600") };
        unsafe { std::env::set_var("QUARTO_HUB_SESSION_ABSOLUTE_SECS", "86400") };
        let lt = SessionLifetimes::from_env();
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_IDLE_SECS") };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_ABSOLUTE_SECS") };
        assert_eq!(
            lt.unwrap(),
            SessionLifetimes {
                idle_secs: 3600,
                absolute_secs: 86400,
            }
        );
    }

    #[test]
    fn lifetimes_from_env_rejects_bad_values() {
        let _guard = ENV_MUTEX.lock().unwrap();
        for (idle, absolute) in [
            ("abc", "86400"),   // unparsable
            ("0", "86400"),     // non-positive
            ("-5", "86400"),    // negative
            ("86401", "86400"), // idle > absolute
        ] {
            unsafe { std::env::set_var("QUARTO_HUB_SESSION_IDLE_SECS", idle) };
            unsafe { std::env::set_var("QUARTO_HUB_SESSION_ABSOLUTE_SECS", absolute) };
            let lt = SessionLifetimes::from_env();
            assert!(
                lt.is_err(),
                "idle={idle} absolute={absolute} must be rejected"
            );
        }
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_IDLE_SECS") };
        unsafe { std::env::remove_var("QUARTO_HUB_SESSION_ABSOLUTE_SECS") };
    }

    #[test]
    fn expiry_is_idle_bound_early_in_session() {
        let lt = SessionLifetimes::default();
        let auth_time = 1_000_000;
        // Right after login the idle window ends first.
        assert_eq!(
            lt.expiry(auth_time, auth_time),
            auth_time + lt.idle_secs,
            "fresh session: exp = now + idle"
        );
    }

    #[test]
    fn expiry_is_capped_by_absolute_late_in_session() {
        let lt = SessionLifetimes::default();
        let auth_time = 1_000_000;
        // 25 days in: now + 7d idle would exceed auth_time + 30d.
        let late = auth_time + 25 * 24 * 3600;
        assert_eq!(
            lt.expiry(late, auth_time),
            auth_time + lt.absolute_secs,
            "late re-issue: exp capped at auth_time + absolute"
        );
    }

    // ── mint + verify roundtrip (C2) ──────────────────────────────

    #[test]
    fn mint_verify_roundtrip_preserves_identity_claims() {
        let t = now();
        let token = mint_session(&keys(), SessionLifetimes::default(), &identity(), t).unwrap();
        let v = verify_session(&keys(), SessionLifetimes::default(), &token, t).unwrap();
        assert_eq!(v.claims.iss, SESSION_ISSUER);
        assert_eq!(v.claims.sub, "google-sub-123");
        assert_eq!(v.claims.email, "user@posit.co");
        assert!(v.claims.email_verified);
        assert_eq!(v.claims.name.as_deref(), Some("Test User"));
        assert_eq!(
            v.claims.picture.as_deref(),
            Some("https://lh3.googleusercontent.com/p")
        );
        assert_eq!(v.claims.iat, t);
        assert_eq!(v.claims.auth_time, t, "fresh mint anchors auth_time = now");
        assert_eq!(v.claims.exp, t + DEFAULT_SESSION_IDLE_SECS);
        assert!(v.signed_with_current_key);
    }

    #[test]
    fn mint_session_at_returns_the_sid_inside_the_token() {
        // H5 logs the returned `sid` as the identity of the new session
        // family. If it ever drifted from the token's claim, the audit
        // trail would name a session that does not exist.
        let t = now();
        let lt = SessionLifetimes::default();
        let minted = mint_session_at(&keys(), lt, &identity(), t, t).unwrap();
        let v = verify_session(&keys(), lt, &minted.token, t).unwrap();
        assert_eq!(minted.sid, v.claims.sid);
    }

    #[test]
    fn mint_stamps_current_kid_in_header() {
        let token = mint_session(&keys(), SessionLifetimes::default(), &identity(), now()).unwrap();
        let header = jsonwebtoken::decode_header(&token).unwrap();
        assert_eq!(header.alg, jsonwebtoken::Algorithm::HS256);
        assert_eq!(header.kid.as_deref(), Some(keys().current().kid()));
    }

    #[test]
    fn mint_generates_fresh_sid_per_login() {
        let t = now();
        let k = keys();
        let lt = SessionLifetimes::default();
        let t1 = mint_session(&k, lt, &identity(), t).unwrap();
        let t2 = mint_session(&k, lt, &identity(), t).unwrap();
        let s1 = verify_session(&k, lt, &t1, t).unwrap().claims.sid;
        let s2 = verify_session(&k, lt, &t2, t).unwrap().claims.sid;
        assert_eq!(s1.len(), 32, "sid is 16 random bytes hex-encoded");
        assert_ne!(s1, s2, "each login gets a fresh sid");
    }

    #[test]
    fn token_is_compact() {
        // Immune to the >3800-byte cookie drop with plenty of headroom.
        let token = mint_session(&keys(), SessionLifetimes::default(), &identity(), now()).unwrap();
        assert!(
            token.len() < 1024,
            "session token should be compact, got {} bytes",
            token.len()
        );
    }

    // ── expiry + absolute cap (C2) ────────────────────────────────

    #[test]
    fn expired_token_rejected() {
        let t = now();
        let lt = SessionLifetimes::default();
        let token = mint_session(&keys(), lt, &identity(), t).unwrap();
        let past_exp = t + lt.idle_secs + SESSION_LEEWAY_SECS + 1;
        assert_eq!(
            verify_session(&keys(), lt, &token, past_exp),
            Err(SessionVerifyError::Expired)
        );
    }

    #[test]
    fn expiry_leeway_tolerates_small_skew() {
        let t = now();
        let lt = SessionLifetimes::default();
        let token = mint_session(&keys(), lt, &identity(), t).unwrap();
        // 30 s past exp: within the 60 s leeway.
        assert!(verify_session(&keys(), lt, &token, t + lt.idle_secs + 30).is_ok());
    }

    #[test]
    fn absolute_cap_enforced_independently_of_exp() {
        // A token that (buggily) carries a valid signature and a future
        // exp but whose auth_time is past the absolute cap must be
        // rejected — the cap does not trust exp.
        let t = now();
        let lt = SessionLifetimes::default();
        let claims = SessionClaims {
            iss: SESSION_ISSUER.to_string(),
            sub: "s".into(),
            email: "user@posit.co".into(),
            email_verified: true,
            name: None,
            picture: None,
            iat: t,
            auth_time: t - lt.absolute_secs - 1, // past the cap
            exp: t + 600,                        // yet exp is in the future
            sid: "f00df00df00df00df00df00df00df00d".into(),
        };
        let token = sign_claims(&keys(), &claims).unwrap();
        assert_eq!(
            verify_session(&keys(), lt, &token, t),
            Err(SessionVerifyError::AbsoluteCapExceeded)
        );
    }

    // ── tamper + cross-format rejection (C2) ──────────────────────

    #[test]
    fn tampered_payload_rejected() {
        let t = now();
        let lt = SessionLifetimes::default();
        let token = mint_session(&keys(), lt, &identity(), t).unwrap();
        // Bit-flip inside the payload segment.
        let mut parts: Vec<String> = token.split('.').map(String::from).collect();
        let mut payload = parts[1].clone().into_bytes();
        let i = payload.len() / 2;
        payload[i] = if payload[i] == b'A' { b'B' } else { b'A' };
        parts[1] = String::from_utf8(payload).unwrap();
        let tampered = parts.join(".");
        assert_eq!(
            verify_session(&keys(), lt, &tampered, t),
            Err(SessionVerifyError::Tampered)
        );
    }

    #[test]
    fn token_signed_with_wrong_secret_but_matching_kid_rejected() {
        let t = now();
        let lt = SessionLifetimes::default();
        // Forge: sign under SECRET_B but stamp SECRET_A's kid.
        let forged_keys = SessionKeys::new(SECRET_B);
        let claims = SessionClaims {
            iss: SESSION_ISSUER.to_string(),
            sub: "s".into(),
            email: "user@posit.co".into(),
            email_verified: true,
            name: None,
            picture: None,
            iat: t,
            auth_time: t,
            exp: t + 600,
            sid: "f00df00df00df00df00df00df00df00d".into(),
        };
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        header.kid = Some(keys().current().kid().to_string());
        let forged = jsonwebtoken::encode(
            &header,
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(&forged_keys.current().secret),
        )
        .unwrap();
        assert_eq!(
            verify_session(&keys(), lt, &forged, t),
            Err(SessionVerifyError::Tampered)
        );
    }

    #[test]
    fn wrong_issuer_rejected() {
        let t = now();
        let lt = SessionLifetimes::default();
        let claims = SessionClaims {
            iss: "https://accounts.google.com".to_string(),
            sub: "s".into(),
            email: "user@posit.co".into(),
            email_verified: true,
            name: None,
            picture: None,
            iat: t,
            auth_time: t,
            exp: t + 600,
            sid: "f00df00df00df00df00df00df00df00d".into(),
        };
        let token = sign_claims(&keys(), &claims).unwrap();
        assert_eq!(
            verify_session(&keys(), lt, &token, t),
            Err(SessionVerifyError::Tampered)
        );
    }

    /// Algorithm-confusion (C4 review): an asymmetric token stamped
    /// with the hub session `kid` must not reach signature evaluation
    /// under the wrong scheme — the session branch pins `[HS256]`.
    #[test]
    fn rs256_with_hub_kid_rejected() {
        let t = now();
        let lt = SessionLifetimes::default();
        let claims = SessionClaims {
            iss: SESSION_ISSUER.to_string(),
            sub: "s".into(),
            email: "user@posit.co".into(),
            email_verified: true,
            name: None,
            picture: None,
            iat: t,
            auth_time: t,
            exp: t + 600,
            sid: "f00df00df00df00df00df00df00df00d".into(),
        };
        // A syntactically valid JWT whose header says RS256 + our kid.
        // The signature bytes are irrelevant: the pinned algorithm set
        // must reject before any key/signature confusion can matter.
        let header = serde_json::json!({
            "alg": "RS256",
            "typ": "JWT",
            "kid": keys().current().kid(),
        });
        let b64 = |bytes: &[u8]| {
            use base64::Engine as _;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
        };
        let token = format!(
            "{}.{}.{}",
            b64(serde_json::to_string(&header).unwrap().as_bytes()),
            b64(serde_json::to_string(&claims).unwrap().as_bytes()),
            b64(&[0u8; 256]),
        );
        assert_eq!(
            verify_session(&keys(), lt, &token, t),
            Err(SessionVerifyError::Tampered)
        );
    }

    #[test]
    fn garbage_token_rejected_without_panic() {
        let lt = SessionLifetimes::default();
        for garbage in ["", "not-a-jwt", "a.b", "a.b.c.d", "ya29.notajwt"] {
            let r = verify_session(&keys(), lt, garbage, now());
            assert!(r.is_err(), "garbage {garbage:?} must be rejected");
        }
    }

    // ── kid fail-closed (C2) ──────────────────────────────────────

    #[test]
    fn missing_kid_fails_closed() {
        let t = now();
        let lt = SessionLifetimes::default();
        let claims = SessionClaims {
            iss: SESSION_ISSUER.to_string(),
            sub: "s".into(),
            email: "user@posit.co".into(),
            email_verified: true,
            name: None,
            picture: None,
            iat: t,
            auth_time: t,
            exp: t + 600,
            sid: "f00df00df00df00df00df00df00df00d".into(),
        };
        // Correct secret, correct alg — but NO kid in the header.
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        let token = jsonwebtoken::encode(
            &header,
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(&SECRET_A),
        )
        .unwrap();
        assert_eq!(
            verify_session(&keys(), lt, &token, t),
            Err(SessionVerifyError::UnknownKid)
        );
    }

    #[test]
    fn unknown_kid_fails_closed_even_with_valid_signature() {
        let t = now();
        let lt = SessionLifetimes::default();
        let claims = SessionClaims {
            iss: SESSION_ISSUER.to_string(),
            sub: "s".into(),
            email: "user@posit.co".into(),
            email_verified: true,
            name: None,
            picture: None,
            iat: t,
            auth_time: t,
            exp: t + 600,
            sid: "f00df00df00df00df00df00df00df00d".into(),
        };
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
        header.kid = Some("deadbeef".to_string());
        let token = jsonwebtoken::encode(
            &header,
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(&SECRET_A),
        )
        .unwrap();
        assert_eq!(
            verify_session(&keys(), lt, &token, t),
            Err(SessionVerifyError::UnknownKid)
        );
    }

    // ── identity caps (C3) ────────────────────────────────────────

    #[test]
    fn from_oidc_caps_pathological_identity_claims() {
        let claims = crate::auth::OidcClaims {
            sub: "s".into(),
            email: "user@posit.co".into(),
            email_verified: true,
            name: Some("x".repeat(4000)),
            picture: Some(format!("https://example.com/{}", "y".repeat(4000))),
            aud: vec![],
            azp: None,
            iat: None,
            exp: 0,
        };
        let id = SessionIdentity::from_oidc(&claims);
        assert_eq!(id.name.as_ref().unwrap().chars().count(), 200);
        assert!(id.picture.is_none(), "oversized picture URL dropped");

        // Sane values pass through untouched.
        let claims = crate::auth::OidcClaims {
            name: Some("Test User".into()),
            picture: Some("https://lh3.googleusercontent.com/p".into()),
            ..claims
        };
        let id = SessionIdentity::from_oidc(&claims);
        assert_eq!(id.name.as_deref(), Some("Test User"));
        assert_eq!(
            id.picture.as_deref(),
            Some("https://lh3.googleusercontent.com/p")
        );
    }

    // ── re-issue semantics (C3) ───────────────────────────────────

    #[test]
    fn reissue_preserves_auth_time_and_sid_and_advances_iat() {
        let t = now();
        let k = keys();
        let lt = SessionLifetimes::default();
        let token = mint_session(&k, lt, &identity(), t).unwrap();
        let original = verify_session(&k, lt, &token, t).unwrap().claims;

        let later = t + 2 * 3600;
        let reissued = reissue_session(&k, lt, &original, later).unwrap();
        let r = verify_session(&k, lt, &reissued, later).unwrap().claims;

        assert_eq!(r.auth_time, original.auth_time, "auth_time is immutable");
        assert_eq!(r.sid, original.sid, "sid is immutable");
        assert_eq!(r.iat, later, "iat advances to the re-issue instant");
        assert_eq!(r.exp, later + lt.idle_secs, "exp slides from re-issue");
        assert_eq!(r.sub, original.sub);
        assert_eq!(r.email, original.email);
    }

    #[test]
    fn reissue_never_extends_past_absolute_cap() {
        let t = now();
        let k = keys();
        let lt = SessionLifetimes::default();
        let token = mint_session(&k, lt, &identity(), t).unwrap();
        let original = verify_session(&k, lt, &token, t).unwrap().claims;

        // Re-issue 25 days in: exp must be capped at auth_time + 30d,
        // not now + 7d.
        let late = t + 25 * 24 * 3600;
        let reissued = reissue_session(&k, lt, &original, late).unwrap();
        let r = verify_session(&k, lt, &reissued, late).unwrap().claims;
        assert_eq!(r.exp, original.auth_time + lt.absolute_secs);
    }

    #[test]
    fn should_reissue_only_after_min_age() {
        let t = now();
        let k = keys();
        let lt = SessionLifetimes::default();
        let token = mint_session(&k, lt, &identity(), t).unwrap();
        let v = verify_session(&k, lt, &token, t).unwrap();

        assert!(!v.should_reissue(t), "fresh token: no re-issue");
        assert!(
            !v.should_reissue(t + SESSION_REISSUE_MIN_AGE_SECS - 1),
            "younger than 1 h: no re-issue"
        );
        assert!(
            v.should_reissue(t + SESSION_REISSUE_MIN_AGE_SECS),
            "1 h old: re-issue"
        );
    }

    // ── rotation keyring (C5b) ────────────────────────────────────

    #[test]
    fn rotated_keyring_verifies_both_kids_and_never_exceeds_two() {
        let t = now();
        let lt = SessionLifetimes::default();
        let old = SessionKeys::new(SECRET_B);
        let old_token = mint_session(&old, lt, &identity(), t).unwrap();

        let rotated = SessionKeys::with_previous(SECRET_A, SECRET_B);
        // Both kids deterministic and distinct; the ring is exactly
        // current + previous (its type admits no third entry).
        assert_eq!(rotated.current().kid(), derive_session_kid(&SECRET_A));
        assert_eq!(
            rotated.previous().unwrap().kid(),
            derive_session_kid(&SECRET_B)
        );
        assert_ne!(rotated.current().kid(), rotated.previous().unwrap().kid());

        // Old-kid token verifies during the overlap…
        let v = verify_session(&rotated, lt, &old_token, t).unwrap();
        assert!(!v.signed_with_current_key);
        // …and fresh mints carry the new kid.
        let new_token = mint_session(&rotated, lt, &identity(), t).unwrap();
        let header = jsonwebtoken::decode_header(&new_token).unwrap();
        assert_eq!(header.kid.as_deref(), Some(rotated.current().kid()));
    }

    #[test]
    fn old_kid_rejected_without_previous_key() {
        // Post-overlap (previous dropped) and emergency rotation are
        // the same keyring shape: current only — old-kid tokens fail
        // closed as UnknownKid.
        let t = now();
        let lt = SessionLifetimes::default();
        let old = SessionKeys::new(SECRET_B);
        let old_token = mint_session(&old, lt, &identity(), t).unwrap();

        let current_only = SessionKeys::new(SECRET_A);
        assert_eq!(
            verify_session(&current_only, lt, &old_token, t),
            Err(SessionVerifyError::UnknownKid)
        );
    }

    #[test]
    fn should_reissue_immediately_for_non_current_kid() {
        let t = now();
        let lt = SessionLifetimes::default();
        // Minted under SECRET_B, verified by a keyring where SECRET_B
        // is the *previous* key (graceful rotation, C5b shape).
        let old = SessionKeys::new(SECRET_B);
        let token = mint_session(&old, lt, &identity(), t).unwrap();
        let rotated = SessionKeys::with_previous(SECRET_A, SECRET_B);
        let v = verify_session(&rotated, lt, &token, t).unwrap();
        assert!(!v.signed_with_current_key);
        assert!(
            v.should_reissue(t),
            "non-current kid: re-issue regardless of age"
        );
    }
}
