//! Node secret derivation, hashing and tunnel handshake signing (docs §4.1, §9.1).
//!
//! The server never stores a node secret. It is derived deterministically from
//! a server-side pepper (`HMAC(pepper, "interlink-node|device|version")`), so
//! the handshake MAC the local node presents can be re-verified at any time
//! without keeping the secret itself in the database. A leaked database is
//! therefore useless without the pepper, and rotation is just a version bump:
//! the previous version stays valid for [`ROTATION_GRACE_S`] (dual-key window).

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::storage::StorageBackend;
use std::sync::Arc;

/// Meta key holding the server pepper (random, created on first use).
pub const PEPPER_META_KEY: &str = "interlink.node_secret_pepper";
/// Hex length of a node secret (32 bytes).
pub const SECRET_HEX_LEN: usize = 64;
/// Dual-key window after a forced rotation (docs §9.1).
pub const ROTATION_GRACE_S: f64 = 24.0 * 3600.0;

/// Pepper bootstrap: read it from meta, create it once when missing.
///
/// Synchronous on purpose - it is called from a `blocking::run_db` closure.
pub fn ensure_pepper(storage: &Arc<dyn StorageBackend>) -> anyhow::Result<String> {
    if let Some(existing) = storage.get_meta(PEPPER_META_KEY)? {
        if existing.len() >= SECRET_HEX_LEN {
            return Ok(existing);
        }
    }
    let pepper = random_hex(32);
    storage.set_meta(PEPPER_META_KEY, &pepper)?;
    Ok(pepper)
}

/// Generate a fresh random secret for one-time delivery to the device.
pub fn random_secret() -> String {
    random_hex(32)
}

/// Deterministic derivation of the node secret for `(device_id, version)`.
pub fn derive_secret(pepper: &str, device_id: &str, version: i64) -> String {
    hmac_hex(pepper.as_bytes(), &format!("interlink-node|{device_id}|{version}"))
}

/// What the server persists for a node secret (never the secret itself).
pub fn secret_hash(pepper: &str, secret: &str) -> String {
    hmac_hex(&format!("{pepper}#hash"), secret.as_bytes())
}

/// `hmac` field of the `hello` frame: `HMAC_SHA256(node_secret, ticket)`.
pub fn channel_hmac(secret: &str, ticket: &str) -> String {
    hmac_hex(secret.as_bytes(), ticket)
}

/// Handshake verification outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeOutcome {
    /// The MAC matches the derived secret of the presented version.
    Ok,
    /// The presented version is newer than anything issued for the node.
    UnknownVersion,
    /// A retired version is presented outside its dual-key window: either a
    /// node that never picked up the rotation or a replay of an old key
    /// (docs §13.5 20 - refuse and alert).
    StaleVersion,
    /// The version is plausible but the MAC does not match (wrong secret).
    Mismatch,
}

/// Verify a `hello` handshake MAC against the stored secret fingerprint.
///
/// `stored_hash` is `secret_hash(pepper, current_secret)`; it is cross-checked
/// first so a tampered `secret_version` fails fast instead of being derived.
pub fn verify_handshake(
    pepper: &str,
    device_id: &str,
    stored_hash: &str,
    stored_version: i64,
    rotated_at: f64,
    presented_version: i64,
    ticket: &str,
    presented_hmac: &str,
    now: f64,
) -> HandshakeOutcome {
    if presented_version < stored_version {
        let in_grace = stored_version > 1
            && presented_version == stored_version - 1
            && now - rotated_at <= ROTATION_GRACE_S;
        if in_grace {
            // Grace window: the previous key is still accepted (docs §9.1).
            let secret = derive_secret(pepper, device_id, presented_version);
            return compare(&secret, ticket, presented_hmac);
        }
        return HandshakeOutcome::StaleVersion;
    }
    if presented_version > stored_version {
        return HandshakeOutcome::UnknownVersion;
    }
    let secret = derive_secret(pepper, device_id, stored_version);
    // The stored fingerprint must correspond to the derived key.
    if !constant_time_eq(stored_hash.as_bytes(), secret_hash(pepper, &secret).as_bytes()) {
        return HandshakeOutcome::Mismatch;
    }
    compare(&secret, ticket, presented_hmac)
}

/// True while an old secret version is still inside its dual-key window.
pub fn version_in_grace(stored_version: i64, presented_version: i64, rotated_at: f64, now: f64) -> bool {
    presented_version > 1
        && presented_version == stored_version - 1
        && now - rotated_at <= ROTATION_GRACE_S
}

fn compare(secret: &str, ticket: &str, presented: &str) -> HandshakeOutcome {
    let expected = channel_hmac(secret, ticket);
    if constant_time_eq(expected.as_bytes(), presented.as_bytes()) {
        HandshakeOutcome::Ok
    } else {
        HandshakeOutcome::Mismatch
    }
}

pub fn hmac_hex(key: impl AsRef<[u8]>, message: impl AsRef<[u8]>) -> String {
    let mut mac = match Hmac::<Sha256>::new_from_slice(key.as_ref()) {
        Ok(mac) => mac,
        Err(_) => return String::new(),
    };
    mac.update(message.as_ref());
    to_hex(&mac.finalize().into_bytes())
}

/// Hex string of `SHA-256(bytes)`, used for argument/result digests.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    to_hex(&hasher.finalize())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in a.iter().zip(b.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

/// 64-hex-per-32-bytes random value built from UUID v4 entropy (no extra dep).
fn random_hex(bytes: usize) -> String {
    let uuids = bytes.div_ceil(16);
    let mut out = String::with_capacity(bytes * 2);
    for _ in 0..uuids {
        out.push_str(&Uuid::new_v4().simple().to_string());
    }
    out.truncate(bytes * 2);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_secret_is_stable_per_device_and_version() {
        let pepper = "pepper-value";
        let a = derive_secret(pepper, "dev-1", 1);
        let b = derive_secret(pepper, "dev-1", 1);
        assert_eq!(a, b);
        assert_eq!(a.len(), SECRET_HEX_LEN);
        assert_ne!(a, derive_secret(pepper, "dev-2", 1));
        assert_ne!(a, derive_secret(pepper, "dev-1", 2));
    }

    #[test]
    fn handshake_accepts_the_matching_secret_only() {
        let pepper = "pepper-value";
        let version = 3;
        let secret = derive_secret(pepper, "dev-1", version);
        let stored = secret_hash(pepper, &secret);
        let ticket = "itk_ticket";
        let mac = channel_hmac(&secret, ticket);

        let outcome = verify_handshake(
            pepper,
            "dev-1",
            &stored,
            version,
            1_000.0,
            version,
            ticket,
            &mac,
            1_100.0,
        );
        assert_eq!(outcome, HandshakeOutcome::Ok);

        // A replayed MAC for another ticket must fail.
        let other = channel_hmac(&secret, "itk_other");
        assert_eq!(
            verify_handshake(pepper, "dev-1", &stored, version, 1_000.0, version, ticket, &other, 1_100.0),
            HandshakeOutcome::Mismatch
        );
        // Wrong version is never derived.
        assert_eq!(
            verify_handshake(pepper, "dev-1", &stored, version, 1_000.0, version + 1, ticket, &mac, 1_100.0),
            HandshakeOutcome::UnknownVersion
        );
    }

    #[test]
    fn previous_version_is_accepted_only_inside_the_grace_window() {
        let pepper = "pepper-value";
        let rotated_at = 10_000.0;
        let old_secret = derive_secret(pepper, "dev-1", 1);
        let current_hash = secret_hash(pepper, &derive_secret(pepper, "dev-1", 2));
        let ticket = "itk_t";
        let mac = channel_hmac(&old_secret, ticket);

        // 1h after rotation the old key still handshakes...
        assert_eq!(
            verify_handshake(pepper, "dev-1", &current_hash, 2, rotated_at, 1, ticket, &mac, rotated_at + 3_600.0),
            HandshakeOutcome::Ok
        );
        // ...25h later it does not, and the attempt is a stale-key reject.
        assert_eq!(
            verify_handshake(pepper, "dev-1", &current_hash, 2, rotated_at, 1, ticket, &mac, rotated_at + 90_000.0),
            HandshakeOutcome::StaleVersion
        );
        // A key retired before the last rotation is stale immediately, too.
        let older = channel_hmac(&derive_secret(pepper, "dev-1", 1), ticket);
        assert_eq!(
            verify_handshake(pepper, "dev-1", &secret_hash(pepper, &derive_secret(pepper, "dev-1", 3)), 3, rotated_at, 1, ticket, &older, rotated_at + 60.0),
            HandshakeOutcome::StaleVersion,
            "only the single previous version is ever dual-key valid"
        );
    }

    #[test]
    fn tampered_version_column_fails_before_derivation() {
        let pepper = "pepper-value";
        let secret = derive_secret(pepper, "dev-1", 1);
        let mac = channel_hmac(&secret, "itk_t");
        // stored hash belongs to version 2 while the column claims version 1.
        let mismatch = verify_handshake(
            pepper,
            "dev-1",
            &secret_hash(pepper, &derive_secret(pepper, "dev-1", 2)),
            1,
            0.0,
            1,
            "itk_t",
            &mac,
            10.0,
        );
        assert_eq!(mismatch, HandshakeOutcome::Mismatch);
    }

    #[test]
    fn digests_are_hex_and_deterministic() {
        assert_eq!(sha256_hex(b"abc"), sha256_hex(b"abc"));
        assert_eq!(sha256_hex(b"abc").len(), SECRET_HEX_LEN);
    }
}
