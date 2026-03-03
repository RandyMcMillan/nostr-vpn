use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast};

use crate::config::normalize_nostr_pubkey;
use crate::control::PeerAnnouncement;

pub const NOSTR_KIND_NOSTR_VPN: u16 = 31990;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SignalPayload {
    Announce(PeerAnnouncement),
    Disconnect { node_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalEnvelope {
    pub network_id: String,
    pub sender_pubkey: String,
    pub payload: SignalPayload,
}

pub struct NostrSignalingClient {
    network_id: String,
    own_pubkey: String,
    keys: Keys,
    client: Client,
    allowed_senders: Option<HashSet<String>>,
    connected: AtomicBool,
    recv_rx: Mutex<broadcast::Receiver<SignalEnvelope>>,
    recv_tx: broadcast::Sender<SignalEnvelope>,
}

impl NostrSignalingClient {
    pub fn new(network_id: String) -> Result<Self> {
        Self::new_with_keys(network_id, Keys::generate(), Vec::new())
    }

    pub fn from_secret_key(
        network_id: String,
        secret_key: &str,
        participants: Vec<String>,
    ) -> Result<Self> {
        let keys = Keys::parse(secret_key).context("invalid nostr secret key")?;
        Self::new_with_keys(network_id, keys, participants)
    }

    pub fn new_with_keys(
        network_id: String,
        keys: Keys,
        participants: Vec<String>,
    ) -> Result<Self> {
        if network_id.trim().is_empty() {
            return Err(anyhow!("network_id must not be empty"));
        }

        let own_pubkey = keys.public_key().to_hex();

        let client = ClientBuilder::new()
            .signer(keys.clone())
            .database(nostr_sdk::database::MemoryDatabase::new())
            .build();

        let allowed_senders = normalize_participants(participants)?;

        let (recv_tx, recv_rx) = broadcast::channel(2048);

        Ok(Self {
            network_id,
            own_pubkey,
            keys,
            client,
            allowed_senders,
            connected: AtomicBool::new(false),
            recv_rx: Mutex::new(recv_rx),
            recv_tx,
        })
    }

    pub async fn connect(&self, relays: &[String]) -> Result<()> {
        for relay in relays {
            self.client
                .add_relay(relay)
                .await
                .with_context(|| format!("failed to add relay {relay}"))?;
        }

        self.client.connect().await;

        let filter = Filter::new()
            .kind(Kind::Custom(NOSTR_KIND_NOSTR_VPN))
            .since(Timestamp::now() - Duration::from_secs(120));

        self.client
            .subscribe(vec![filter], None)
            .await
            .context("failed to subscribe to nostr-vpn events")?;

        self.start_event_forwarder();
        self.connected.store(true, Ordering::Relaxed);

        Ok(())
    }

    pub async fn disconnect(&self) {
        self.connected.store(false, Ordering::Relaxed);
        let _ = self.client.disconnect().await;
    }

    pub async fn publish(&self, payload: SignalPayload) -> Result<()> {
        if !self.connected.load(Ordering::Relaxed) {
            return Err(anyhow!("client not connected"));
        }

        let envelope = SignalEnvelope {
            network_id: self.network_id.clone(),
            sender_pubkey: self.own_pubkey.clone(),
            payload,
        };

        let content = serde_json::to_string(&envelope).context("failed to serialize envelope")?;
        let tags = vec![Tag::custom(
            TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::N)),
            vec![self.network_id.clone()],
        )];

        let builder = EventBuilder::new(Kind::Custom(NOSTR_KIND_NOSTR_VPN), content, tags);
        let event = builder
            .to_event(&self.keys)
            .context("failed to sign nostr event")?;

        let output = self
            .client
            .send_event(event)
            .await
            .context("failed to publish nostr event")?;

        if output.success.is_empty() {
            return Err(anyhow!("event rejected by all relays"));
        }

        Ok(())
    }

    pub async fn recv(&self) -> Option<SignalEnvelope> {
        let mut rx = self.recv_rx.lock().await;
        loop {
            match rx.recv().await {
                Ok(msg) => return Some(msg),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    fn start_event_forwarder(&self) {
        let mut notifications = self.client.notifications();
        let own_pubkey = self.own_pubkey.clone();
        let network_id = self.network_id.clone();
        let allowed_senders = self.allowed_senders.clone();
        let recv_tx = self.recv_tx.clone();

        tokio::spawn(async move {
            loop {
                let notification = match notifications.recv().await {
                    Ok(notification) => notification,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                };

                if let RelayPoolNotification::Event { event, .. } = notification {
                    if event.kind != Kind::Custom(NOSTR_KIND_NOSTR_VPN) {
                        continue;
                    }

                    if event.pubkey.to_hex() == own_pubkey {
                        continue;
                    }

                    if let Ok(envelope) = serde_json::from_str::<SignalEnvelope>(&event.content) {
                        if envelope.network_id != network_id {
                            continue;
                        }

                        if envelope.sender_pubkey == own_pubkey {
                            continue;
                        }

                        if let Some(allowlist) = &allowed_senders
                            && !allowlist.contains(&envelope.sender_pubkey)
                        {
                            continue;
                        }

                        let _ = recv_tx.send(envelope);
                    }
                }
            }
        });
    }
}

fn normalize_participants(participants: Vec<String>) -> Result<Option<HashSet<String>>> {
    if participants.is_empty() {
        return Ok(None);
    }

    let mut normalized = HashSet::with_capacity(participants.len());
    for participant in participants {
        normalized.insert(normalize_nostr_pubkey(&participant)?);
    }

    if normalized.is_empty() {
        Ok(None)
    } else {
        Ok(Some(normalized))
    }
}
