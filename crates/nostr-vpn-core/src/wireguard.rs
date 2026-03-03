use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceConfig {
    pub private_key: String,
    pub address: String,
    pub listen_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerConfig {
    pub public_key: String,
    pub allowed_ips: String,
    pub endpoint: String,
    pub persistent_keepalive: u16,
}

pub fn render_wireguard_config(interface: &InterfaceConfig, peers: &[PeerConfig]) -> String {
    let mut out = String::new();

    out.push_str("[Interface]\n");
    out.push_str(&format!("PrivateKey = {}\n", interface.private_key));
    out.push_str(&format!("Address = {}\n", interface.address));
    out.push_str(&format!("ListenPort = {}\n", interface.listen_port));

    for peer in peers {
        out.push_str("\n[Peer]\n");
        out.push_str(&format!("PublicKey = {}\n", peer.public_key));
        out.push_str(&format!("AllowedIPs = {}\n", peer.allowed_ips));
        out.push_str(&format!("Endpoint = {}\n", peer.endpoint));
        out.push_str(&format!(
            "PersistentKeepalive = {}\n",
            peer.persistent_keepalive
        ));
    }

    out
}
