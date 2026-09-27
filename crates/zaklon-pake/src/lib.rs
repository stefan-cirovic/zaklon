//! Pairing a phone with a hub it found on the network ("Find hubs").
//!
//! A discovery answer is not authenticated: any device on the Wi-Fi can
//! answer with the hub's name and certificate fingerprint. So the phone does
//! not take the certificate from it. It connects accepting any certificate,
//! notes the one it got, and runs SPAKE2 with the hub, using the 6-digit
//! pairing code shown on the laptop as the password. The two sides end up
//! with the same key only if they used the same code. Someone who does not
//! know the code gets one guess per run (the hub allows three runs per code),
//! and what they see of a run does not let them test other codes offline.
//!
//! With that key the hub proves which certificate is its own, and the phone
//! checks the proof against the certificate it actually saw. A device in
//! between (one that answers with its own certificate and passes the
//! messages on to the real hub) cannot make that proof for its certificate,
//! so the phone stops before it sends anything else. Only then does the phone
//! prove that it has the key too, and send the household password over a
//! connection pinned to that certificate.
//!
//! The messages (all binary values as lower-case hex):
//! 1. phone to hub, [`START_PATH`], [`StartRequest`]: the phone's SPAKE2 message.
//! 2. hub to phone, [`StartReply`]: the hub's SPAKE2 message; `check`, a MAC
//!    over both messages (it tells a wrong code apart); and `proof`, a MAC
//!    over the hub's certificate fingerprint and both messages.
//! 3. phone to hub, [`FINISH_PATH`], [`FinishRequest`]: the phone's MAC over
//!    the fingerprint and both messages, with the pairing request itself.
//!
//! The MAC keys are derived from the SPAKE2 key with HKDF-SHA256, a
//! different one for each MAC, so no MAC can be passed off as another.

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use subtle::ConstantTimeEq;

/// Where the phone sends its first message.
pub const START_PATH: &str = "/api/pair/pake/start";
/// Where the phone sends its proof and the pairing request.
pub const FINISH_PATH: &str = "/api/pair/pake/finish";

/// SHA-256 of a DER certificate.
pub type Fingerprint = [u8; 32];
/// An HMAC-SHA256 value.
pub type Tag = [u8; 32];

/// Names of the two sides, part of the SPAKE2 key. The version keeps runs of
/// a future, different protocol from ever agreeing with this one.
const PHONE_ID: &[u8] = b"zaklon-pair-v1 phone";
const HUB_ID: &[u8] = b"zaklon-pair-v1 hub";
const SALT: &[u8] = b"zaklon-pair-v1";

/// Why a run did not succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A message or MAC is not in the expected form.
    BadMessage,
    /// The two sides used different codes.
    WrongCode,
    /// The code was right, but the hub's proof is not for the certificate
    /// the phone saw: something in between passed the messages on.
    NotTheHub,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refusal::BadMessage => "bad pairing message",
            Refusal::WrongCode => "wrong pairing code",
            Refusal::NotTheHub => "another device answered in place of the hub; the password was not sent",
        })
    }
}

impl std::error::Error for Refusal {}

/// Step 1, phone to hub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRequest {
    pub msg: String,
}

/// Step 2, hub to phone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartReply {
    /// Names this run in step 3.
    pub session: String,
    pub msg: String,
    pub check: String,
    pub proof: String,
}

/// Step 3, phone to hub. (No `Debug`: it carries the password.)
#[derive(Clone, Serialize, Deserialize)]
pub struct FinishRequest {
    pub session: String,
    pub proof: String,
    pub password: String,
    pub device_name: String,
    #[serde(default)]
    pub platform: Option<String>,
    /// Random value chosen by the phone; repeating the same request with the
    /// same nonce returns the same result instead of failing.
    #[serde(default)]
    pub nonce: Option<String>,
}

/// The three MAC keys of one run.
struct Keys {
    check: [u8; 32],
    hub: [u8; 32],
    phone: [u8; 32],
}

fn keys(shared: &[u8]) -> Keys {
    let hk = Hkdf::<Sha256>::new(Some(SALT), shared);
    let expand = |info: &[u8]| {
        let mut out = [0u8; 32];
        hk.expand(info, &mut out).expect("32 bytes is a valid HKDF-SHA256 length");
        out
    };
    Keys { check: expand(b"code check"), hub: expand(b"hub proof"), phone: expand(b"phone proof") }
}

fn mac_over(key: &[u8; 32], parts: &[&[u8]]) -> Hmac<Sha256> {
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    for p in parts {
        m.update(p);
    }
    m
}

fn mac(key: &[u8; 32], parts: &[&[u8]]) -> Tag {
    mac_over(key, parts).finalize().into_bytes().into()
}

/// Compares in constant time.
fn mac_matches(key: &[u8; 32], parts: &[&[u8]], tag: &[u8]) -> bool {
    mac_over(key, parts).verify_slice(tag).is_ok()
}

/// The hub's side of one run: its answer to the phone's first message.
pub struct HubAnswer {
    /// The hub's SPAKE2 message.
    pub msg: Vec<u8>,
    /// MAC over both messages: lets the phone tell a wrong code apart.
    pub check: Tag,
    /// MAC over the hub's certificate fingerprint and both messages.
    pub proof: Tag,
    /// What the phone must send back in step 3 (compare with [`proof_matches`]).
    pub expect: Tag,
}

/// Answer the phone's first message, for the open `code` and the hub's own
/// certificate fingerprint. Both SPAKE2 messages have a fixed length (checked
/// by `finish`), so writing one after the other is unambiguous.
pub fn hub_answer(code: &str, phone_msg: &[u8], own: &Fingerprint) -> Result<HubAnswer, Refusal> {
    let (spake, msg) =
        Spake2::<Ed25519Group>::start_b(&Password::new(code.as_bytes()), &Identity::new(PHONE_ID), &Identity::new(HUB_ID));
    let shared = spake.finish(phone_msg).map_err(|_| Refusal::BadMessage)?;
    let k = keys(&shared);
    Ok(HubAnswer {
        check: mac(&k.check, &[phone_msg, &msg]),
        proof: mac(&k.hub, &[own, phone_msg, &msg]),
        expect: mac(&k.phone, &[own, phone_msg, &msg]),
        msg,
    })
}

/// Whether the phone's proof is the expected one, compared in constant time.
pub fn proof_matches(expect: &Tag, got: &[u8]) -> bool {
    bool::from(expect[..].ct_eq(got))
}

/// The phone's side of one run.
pub struct Phone {
    spake: Spake2<Ed25519Group>,
    msg: Vec<u8>,
}

impl Phone {
    /// Start a run with the code the person typed.
    pub fn start(code: &str) -> Self {
        let (spake, msg) =
            Spake2::<Ed25519Group>::start_a(&Password::new(code.as_bytes()), &Identity::new(PHONE_ID), &Identity::new(HUB_ID));
        Self { spake, msg }
    }

    /// The first message, for the hub.
    pub fn message(&self) -> &[u8] {
        &self.msg
    }

    /// Check the hub's answer against `seen`, the fingerprint of the
    /// certificate on the connection that brought the answer. Returns the
    /// phone's proof for step 3. Nothing else may be sent to the hub unless
    /// this succeeds.
    pub fn check(self, hub_msg: &[u8], check: &[u8], proof: &[u8], seen: &Fingerprint) -> Result<Tag, Refusal> {
        let Phone { spake, msg } = self;
        let shared = spake.finish(hub_msg).map_err(|_| Refusal::BadMessage)?;
        let k = keys(&shared);
        if !mac_matches(&k.check, &[&msg, hub_msg], check) {
            return Err(Refusal::WrongCode);
        }
        if !mac_matches(&k.hub, &[seen, &msg, hub_msg], proof) {
            return Err(Refusal::NotTheHub);
        }
        Ok(mac(&k.phone, &[seen, &msg, hub_msg]))
    }
}

/// SHA-256 of a DER certificate, as the phone pins it.
pub fn fingerprint_of(der: &[u8]) -> Fingerprint {
    Sha256::digest(der).into()
}

/// A fingerprint written as 64 hex digits (how the hub and the phone store it).
pub fn fingerprint_from_hex(s: &str) -> Option<Fingerprint> {
    from_hex(s)?.try_into().ok()
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex digits (either case) as bytes; `None` for anything else.
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim().as_bytes();
    if !s.len().is_multiple_of(2) || !s.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
    Some(s.chunks(2).map(|p| (digit(p[0]) << 4) | digit(p[1])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUB_FP: Fingerprint = [7; 32];
    const OTHER_FP: Fingerprint = [9; 32];

    /// One run: the phone with `typed`, the hub with `code`; the phone sees `seen`.
    fn run(typed: &str, code: &str, seen: &Fingerprint) -> (Result<Tag, Refusal>, HubAnswer) {
        let phone = Phone::start(typed);
        let answer = hub_answer(code, phone.message(), &HUB_FP).unwrap();
        (phone.check(&answer.msg, &answer.check, &answer.proof, seen), answer)
    }

    #[test]
    fn the_same_code_and_certificate_pair() {
        let (proof, answer) = run("042917", "042917", &HUB_FP);
        let proof = proof.expect("the hub proves itself");
        assert!(proof_matches(&answer.expect, &proof), "and the phone proves the code");
        // A proof of one side is never accepted for the other.
        assert!(!proof_matches(&answer.expect, &answer.proof));
        assert_ne!(answer.check, answer.proof);
    }

    #[test]
    fn a_wrong_code_is_told_apart() {
        let (result, answer) = run("042918", "042917", &HUB_FP);
        assert_eq!(result, Err(Refusal::WrongCode));
        // Whatever the phone sends back, the hub does not accept it.
        let (other, _) = run("042918", "042918", &HUB_FP);
        assert!(!proof_matches(&answer.expect, &other.unwrap()));
    }

    #[test]
    fn a_device_in_between_is_caught() {
        // The right code went through a device with another certificate:
        // the code checks out, the certificate does not.
        let (result, _) = run("042917", "042917", &OTHER_FP);
        assert_eq!(result, Err(Refusal::NotTheHub));
    }

    #[test]
    fn a_changed_answer_is_refused() {
        let phone = Phone::start("042917");
        let answer = hub_answer("042917", phone.message(), &HUB_FP).unwrap();
        let mut proof = answer.proof;
        proof[0] ^= 1;
        assert_eq!(phone.check(&answer.msg, &answer.check, &proof, &HUB_FP), Err(Refusal::NotTheHub));

        let phone = Phone::start("042917");
        let answer = hub_answer("042917", phone.message(), &HUB_FP).unwrap();
        assert_eq!(phone.check(&answer.msg, &answer.check[..31], &answer.proof, &HUB_FP), Err(Refusal::WrongCode));

        // The hub's message sent back as the phone's (a reflection), or cut short.
        let phone = Phone::start("042917");
        let answer = hub_answer("042917", phone.message(), &HUB_FP).unwrap();
        assert!(hub_answer("042917", &answer.msg, &HUB_FP).is_err());
        assert_eq!(phone.check(&answer.msg[..20], &answer.check, &answer.proof, &HUB_FP), Err(Refusal::BadMessage));
        assert!(hub_answer("042917", b"", &HUB_FP).is_err());
    }

    #[test]
    fn every_run_is_new() {
        let a = Phone::start("042917");
        let b = Phone::start("042917");
        assert_ne!(a.message(), b.message());
        let (x, y) = (hub_answer("042917", a.message(), &HUB_FP).unwrap(), hub_answer("042917", a.message(), &HUB_FP).unwrap());
        assert_ne!(x.msg, y.msg);
        assert_ne!(x.expect, y.expect);
    }

    #[test]
    fn proofs_are_compared_whole() {
        let t = [5u8; 32];
        assert!(proof_matches(&t, &[5; 32]));
        assert!(!proof_matches(&t, &[5; 31]));
        assert!(!proof_matches(&t, &[5; 33]));
        assert!(!proof_matches(&t, b""));
    }

    #[test]
    fn hex_roundtrip() {
        let fp: Fingerprint = core::array::from_fn(|i| (i * 37) as u8);
        let text = to_hex(&fp);
        assert_eq!(text.len(), 64);
        assert_eq!(fingerprint_from_hex(&text), Some(fp));
        assert_eq!(fingerprint_from_hex(&text.to_uppercase()), Some(fp));
        assert_eq!(fingerprint_from_hex(&text[..62]), None);
        assert_eq!(from_hex("0g"), None);
        assert_eq!(from_hex("+f"), None);
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("čš"), None);
        assert_eq!(from_hex(""), Some(vec![]));
        assert_eq!(fingerprint_of(b"x"), fingerprint_of(b"x"));
    }
}
