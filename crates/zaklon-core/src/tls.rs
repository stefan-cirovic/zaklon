//! Self-signed TLS identity for the hub. Generated once on first run and
//! stored under `<root>/household/tls/`. Phones pin the certificate
//! fingerprint they receive in the pairing QR, so the certificate never
//! needs to be trusted by the system store.

use std::path::Path;

use anyhow::{Context, Result};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct Identity {
    pub cert_pem: String,
    pub key_pem: String,
    /// Lower-case hex SHA-256 of the DER certificate.
    pub fingerprint: String,
}

impl Identity {
    /// Fingerprint formatted as `AB:CD:...` for display.
    pub fn fingerprint_display(&self) -> String {
        self.fingerprint
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap_or("??").to_uppercase())
            .collect::<Vec<_>>()
            .join(":")
    }
}

/// Load the identity from `dir`, or generate and persist a new one.
pub fn load_or_generate(dir: &Path, hub_name: &str) -> Result<Identity> {
    let cert_path = dir.join("hub-cert.pem");
    let key_path = dir.join("hub-key.pem");
    if cert_path.exists() && key_path.exists() {
        let cert_pem = std::fs::read_to_string(&cert_path).context("reading hub-cert.pem")?;
        let key_pem = std::fs::read_to_string(&key_path).context("reading hub-key.pem")?;
        let fingerprint = hex(&Sha256::digest(pem_to_der(&cert_pem)?));
        return Ok(Identity { cert_pem, key_pem, fingerprint });
    }
    // The key is written first, so a key alone is a first start that was cut
    // short: no phone has seen that certificate yet. A certificate alone
    // (damaged by hand) cannot serve without its key, so phones pair again.
    if cert_path.exists() {
        tracing::warn!("hub-key.pem is missing; making a new identity, so phones must pair again");
    }

    let mut params = CertificateParams::new(vec!["localhost".to_string(), "zaklon.local".to_string()])
        .context("certificate params")?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, hub_name);
    dn.push(DnType::OrganizationName, "Zaklon hub");
    params.distinguished_name = dn;
    params.subject_alt_names.push(SanType::DnsName("localhost".try_into()?));
    // Ten years: the fingerprint is pinned by phones, so rotation is a re-pair.
    params.not_before = rcgen::date_time_ymd(2026, 1, 1);
    params.not_after = rcgen::date_time_ymd(2036, 12, 31);

    let key_pair = KeyPair::generate().context("generating key pair")?;
    let cert = params.self_signed(&key_pair).context("self-signing certificate")?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    let fingerprint = hex(&Sha256::digest(cert.der()));

    // Each file whole or not at all, the key before the certificate: a
    // certificate on disk always has its key next to it.
    std::fs::create_dir_all(dir)?;
    crate::config::write_atomic(&key_path, key_pem.as_bytes()).context("writing hub-key.pem")?;
    crate::config::write_atomic(&cert_path, cert_pem.as_bytes()).context("writing hub-cert.pem")?;
    Ok(Identity { cert_pem, key_pem, fingerprint })
}

fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(body.trim())
        .context("decoding certificate PEM")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_is_made_once_and_kept() {
        let dir = std::env::temp_dir().join(format!("zaklon-tls-{}", uuid::Uuid::new_v4()));
        let first = load_or_generate(&dir, "Zaklon test").unwrap();
        let again = load_or_generate(&dir, "Another name").unwrap();
        assert_eq!(again.fingerprint, first.fingerprint, "phones stay paired across restarts");
        assert_eq!(again.key_pem, first.key_pem);
        // The fingerprint is the SHA-256 of the certificate phones receive.
        let der = pem_to_der(&std::fs::read_to_string(dir.join("hub-cert.pem")).unwrap()).unwrap();
        assert_eq!(first.fingerprint, hex(&Sha256::digest(&der)));
        assert_eq!(first.fingerprint.len(), 64);
        assert_eq!(first.fingerprint_display().len(), 32 * 3 - 1);
        assert!(!dir.join("hub-key.tmp").exists() && !dir.join("hub-cert.tmp").exists());

        // A key without its certificate (a first start cut short) is replaced.
        std::fs::remove_file(dir.join("hub-cert.pem")).unwrap();
        let fresh = load_or_generate(&dir, "Zaklon test").unwrap();
        assert_ne!(fresh.fingerprint, first.fingerprint);
        assert_eq!(load_or_generate(&dir, "Zaklon test").unwrap().fingerprint, fresh.fingerprint);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
