use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPair {
    pub private_key: String,
    pub public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeTranscript {
    pub initiation_len: usize,
    pub response_len: usize,
    pub keepalive_len: usize,
}

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid key encoding")]
    InvalidEncoding,
    #[error("invalid key length")]
    InvalidLength,
    #[error("unexpected boringtun handshake result")]
    UnexpectedHandshakeResult,
}

pub fn generate_keypair() -> KeyPair {
    let private_key = StaticSecret::random_from_rng(OsRng);
    let public_key = PublicKey::from(&private_key);

    KeyPair {
        private_key: STANDARD.encode(private_key.to_bytes()),
        public_key: STANDARD.encode(public_key.as_bytes()),
    }
}

pub fn decode_private_key(encoded: &str) -> Result<StaticSecret, CryptoError> {
    let raw = STANDARD
        .decode(encoded)
        .map_err(|_| CryptoError::InvalidEncoding)?;

    let bytes: [u8; 32] = raw.try_into().map_err(|_| CryptoError::InvalidLength)?;
    Ok(StaticSecret::from(bytes))
}

pub fn decode_public_key(encoded: &str) -> Result<PublicKey, CryptoError> {
    let raw = STANDARD
        .decode(encoded)
        .map_err(|_| CryptoError::InvalidEncoding)?;

    let bytes: [u8; 32] = raw.try_into().map_err(|_| CryptoError::InvalidLength)?;
    Ok(PublicKey::from(bytes))
}

pub fn public_key_from_private_key(private_key: &StaticSecret) -> String {
    STANDARD.encode(PublicKey::from(private_key).as_bytes())
}

pub fn simulate_boringtun_handshake(
    initiator_private_key: &str,
    responder_private_key: &str,
) -> Result<HandshakeTranscript, CryptoError> {
    let initiator_private = decode_private_key(initiator_private_key)?;
    let responder_private = decode_private_key(responder_private_key)?;

    let initiator_public = PublicKey::from(&initiator_private);
    let responder_public = PublicKey::from(&responder_private);

    let mut initiator = Tunn::new(initiator_private, responder_public, None, Some(25), 1, None);
    let mut responder = Tunn::new(responder_private, initiator_public, None, Some(25), 2, None);

    let mut initiator_buf = [0_u8; 2048];
    let mut responder_buf = [0_u8; 2048];

    let initiation = match initiator.format_handshake_initiation(&mut initiator_buf, false) {
        TunnResult::WriteToNetwork(packet) => packet.to_vec(),
        _ => return Err(CryptoError::UnexpectedHandshakeResult),
    };

    let response = match responder.decapsulate(None, &initiation, &mut responder_buf) {
        TunnResult::WriteToNetwork(packet) => packet.to_vec(),
        _ => return Err(CryptoError::UnexpectedHandshakeResult),
    };

    let keepalive = match initiator.decapsulate(None, &response, &mut initiator_buf) {
        TunnResult::WriteToNetwork(packet) => packet.to_vec(),
        _ => return Err(CryptoError::UnexpectedHandshakeResult),
    };

    let final_result = responder.decapsulate(None, &keepalive, &mut responder_buf);
    if !matches!(
        final_result,
        TunnResult::Done | TunnResult::WriteToNetwork(_)
    ) {
        return Err(CryptoError::UnexpectedHandshakeResult);
    }

    Ok(HandshakeTranscript {
        initiation_len: initiation.len(),
        response_len: response.len(),
        keepalive_len: keepalive.len(),
    })
}
