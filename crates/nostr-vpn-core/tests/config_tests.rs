use nostr_sdk::prelude::{Keys, ToBech32};
use nostr_vpn_core::config::{
    AppConfig, DEFAULT_RELAYS, derive_network_id_from_participants, normalize_nostr_pubkey,
};

#[test]
fn default_relays_match_hashtree_defaults() {
    assert_eq!(
        DEFAULT_RELAYS,
        [
            "wss://temp.iris.to",
            "wss://relay.damus.io",
            "wss://nos.lol",
            "wss://relay.primal.net",
            "wss://offchain.pub",
        ]
    );
}

#[test]
fn network_id_derivation_is_order_independent() {
    let left =
        derive_network_id_from_participants(&["b".to_string(), "a".to_string(), "c".to_string()]);
    let right =
        derive_network_id_from_participants(&["c".to_string(), "b".to_string(), "a".to_string()]);

    assert_eq!(left, right);
}

#[test]
fn generated_config_auto_populates_keys() {
    let config = AppConfig::generated();

    assert!(!config.node.private_key.is_empty());
    assert!(!config.node.public_key.is_empty());
    assert!(!config.nostr.secret_key.is_empty());
    assert!(!config.nostr.public_key.is_empty());
    assert!(!config.nostr.relays.is_empty());
}

#[test]
fn participants_are_normalized_to_hex_pubkeys() {
    let keys = Keys::generate();
    let npub = keys.public_key().to_bech32().expect("npub");
    let hex = keys.public_key().to_hex();

    let mut config = AppConfig::generated();
    config.participants = vec![npub, hex.clone()];
    config.ensure_defaults();

    assert_eq!(config.participants, vec![hex]);
}

#[test]
fn normalize_accepts_npub() {
    let keys = Keys::generate();
    let npub = keys.public_key().to_bech32().expect("npub");

    let normalized = normalize_nostr_pubkey(&npub).expect("normalize npub");

    assert_eq!(normalized, keys.public_key().to_hex());
}
