mod support;

use std::time::Duration;

use nostr_vpn_core::control::PeerAnnouncement;
use nostr_vpn_core::signaling::{NostrSignalingClient, SignalPayload};
use tokio::time::timeout;

use crate::support::ws_relay::WsRelay;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn announces_over_local_nostr_relay() {
    let mut relay = WsRelay::new();
    relay.start().await.expect("relay should start");
    let relay_url = relay.url().expect("relay url");

    let network_id = "nostr-vpn-test".to_string();

    let sender = NostrSignalingClient::new(network_id.clone()).expect("sender client");
    let receiver = NostrSignalingClient::new(network_id).expect("receiver client");

    sender
        .connect(std::slice::from_ref(&relay_url))
        .await
        .expect("sender connect");
    receiver
        .connect(&[relay_url])
        .await
        .expect("receiver connect");

    tokio::time::sleep(Duration::from_millis(200)).await;

    let announcement = PeerAnnouncement {
        node_id: "sender-node".to_string(),
        public_key: "sender-public".to_string(),
        endpoint: "127.0.0.1:51820".to_string(),
        tunnel_ip: "10.44.0.5/32".to_string(),
        timestamp: 42,
    };

    sender
        .publish(SignalPayload::Announce(announcement.clone()))
        .await
        .expect("publish should succeed");

    let received = timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("timed out waiting for message")
        .expect("message expected");

    assert_eq!(received.network_id, "nostr-vpn-test");
    assert_eq!(received.payload, SignalPayload::Announce(announcement));

    sender.disconnect().await;
    receiver.disconnect().await;
    relay.stop().await;
}
