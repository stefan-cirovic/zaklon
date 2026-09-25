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
    pub cert_der: Vec<u8>,
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
        let cert_der = pem_to_der(&cert_pem)?;
        let fingerprint = hex(&Sha256::digest(&cert_der));
        return Ok(Identity { cert_pem, key_pem, cert_der, fingerprint });
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
    let cert_der = cert.der().to_vec();
    let fingerprint = hex(&Sha256::digest(&cert_der));

    std::fs::create_dir_all(dir)?;
    std::fs::write(&cert_path, &cert_pem).context("writing hub-cert.pem")?;
    std::fs::write(&key_path, &key_pem).context("writing hub-key.pem")?;
    Ok(Identity { cert_pem, key_pem, cert_der, fingerprint })
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
