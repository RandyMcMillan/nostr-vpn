use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use nostr_vpn_core::config::{
    AppConfig, DEFAULT_RELAYS, derive_network_id_from_participants, normalize_nostr_pubkey,
};
use nostr_vpn_core::control::PeerAnnouncement;
use nostr_vpn_core::crypto::generate_keypair;
use nostr_vpn_core::signaling::{NostrSignalingClient, SignalPayload};
use nostr_vpn_core::wireguard::{InterfaceConfig, PeerConfig, render_wireguard_config};

#[derive(Debug, Parser)]
#[command(name = "nostr-vpn")]
#[command(about = "Nostr-signaled WireGuard control plane built on boringtun")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize a local config file (keys are generated automatically).
    Init {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        force: bool,
        /// Participant Nostr pubkeys (npub or hex) that define the network.
        #[arg(long = "participant")]
        participants: Vec<String>,
    },
    /// Generate a boringtun-compatible keypair.
    Keygen {
        #[arg(long)]
        json: bool,
    },
    /// Broadcast this node's announcement over Nostr.
    Announce {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        network_id: Option<String>,
        #[arg(long = "participant")]
        participants: Vec<String>,
        #[arg(long)]
        node_id: Option<String>,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        tunnel_ip: Option<String>,
        #[arg(long)]
        public_key: Option<String>,
        #[arg(long)]
        relay: Vec<String>,
    },
    /// Listen for peer announcements.
    Listen {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        network_id: Option<String>,
        #[arg(long = "participant")]
        participants: Vec<String>,
        #[arg(long)]
        relay: Vec<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Render a WireGuard config from local values and peer tuples.
    RenderWg {
        #[arg(long)]
        config: Option<PathBuf>,
        /// Format: <public_key>,<allowed_ips>,<endpoint>
        #[arg(long = "peer")]
        peers: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let cli = Cli::parse();

    match cli.command {
        Command::Init {
            config,
            force,
            participants,
        } => {
            let path = config.unwrap_or_else(default_config_path);
            init_config(&path, force, participants)?;
        }
        Command::Keygen { json } => {
            let pair = generate_keypair();
            if json {
                println!("{}", serde_json::to_string_pretty(&pair)?);
            } else {
                println!("private_key={}", pair.private_key);
                println!("public_key={}", pair.public_key);
            }
        }
        Command::Announce {
            config,
            network_id,
            participants,
            node_id,
            endpoint,
            tunnel_ip,
            public_key,
            relay,
        } => {
            let config_path = config.unwrap_or_else(default_config_path);
            let mut app = load_or_default_config(&config_path)?;

            apply_participants_override(&mut app, participants)?;
            if let Some(network_id) = network_id {
                app.network_id = network_id;
            }

            let network_id = app.effective_network_id();
            let node_id = node_id.unwrap_or_else(|| app.node.id.clone());
            let endpoint = endpoint.unwrap_or_else(|| app.node.endpoint.clone());
            let tunnel_ip = tunnel_ip.unwrap_or_else(|| app.node.tunnel_ip.clone());
            let public_key = public_key.unwrap_or_else(|| app.node.public_key.clone());
            let relays = resolve_relays(&relay, &app);

            let client = NostrSignalingClient::from_secret_key(
                network_id.clone(),
                &app.nostr.secret_key,
                app.participant_pubkeys_hex(),
            )?;
            client.connect(&relays).await?;

            client
                .publish(SignalPayload::Announce(PeerAnnouncement {
                    node_id,
                    public_key,
                    endpoint,
                    tunnel_ip,
                    timestamp: unix_timestamp(),
                }))
                .await
                .context("failed to publish announcement")?;

            client.disconnect().await;
            println!(
                "announced on {} relays for network {network_id}",
                relays.len()
            );
        }
        Command::Listen {
            config,
            network_id,
            participants,
            relay,
            limit,
        } => {
            let config_path = config.unwrap_or_else(default_config_path);
            let mut app = load_or_default_config(&config_path)?;

            apply_participants_override(&mut app, participants)?;
            if let Some(network_id) = network_id {
                app.network_id = network_id;
            }

            let network_id = app.effective_network_id();
            let relays = resolve_relays(&relay, &app);

            let client = NostrSignalingClient::from_secret_key(
                network_id.clone(),
                &app.nostr.secret_key,
                app.participant_pubkeys_hex(),
            )?;
            client.connect(&relays).await?;

            let mut seen = 0_usize;
            loop {
                let Some(message) = client.recv().await else {
                    break;
                };

                println!("{}", serde_json::to_string_pretty(&message)?);

                seen += 1;
                if let Some(limit) = limit
                    && seen >= limit
                {
                    break;
                }
            }

            client.disconnect().await;
        }
        Command::RenderWg { config, peers } => {
            let config_path = config.unwrap_or_else(default_config_path);
            let app = load_or_default_config(&config_path)?;

            let interface = InterfaceConfig {
                private_key: app.node.private_key.clone(),
                address: app.node.tunnel_ip.clone(),
                listen_port: app.node.listen_port,
            };

            let parsed_peers = peers
                .iter()
                .map(|value| parse_peer_arg(value))
                .collect::<Result<Vec<_>>>()?;

            print!("{}", render_wireguard_config(&interface, &parsed_peers));
        }
    }

    Ok(())
}

fn parse_peer_arg(value: &str) -> Result<PeerConfig> {
    let mut parts = value.split(',');
    let public_key = parts.next().unwrap_or_default().trim().to_string();
    let allowed_ips = parts.next().unwrap_or_default().trim().to_string();
    let endpoint = parts.next().unwrap_or_default().trim().to_string();

    if public_key.is_empty() || allowed_ips.is_empty() || endpoint.is_empty() {
        return Err(anyhow!(
            "invalid --peer format, expected <public_key>,<allowed_ips>,<endpoint>"
        ));
    }

    Ok(PeerConfig {
        public_key,
        allowed_ips,
        endpoint,
        persistent_keepalive: 25,
    })
}

fn init_config(path: &Path, force: bool, participants: Vec<String>) -> Result<()> {
    if path.exists() && !force {
        return Err(anyhow!(
            "config already exists at {} (pass --force to overwrite)",
            path.display()
        ));
    }

    let mut config = AppConfig::generated();
    apply_participants_override(&mut config, participants)?;
    config.save(path)?;

    println!("wrote {}", path.display());
    println!("network_id={}", config.effective_network_id());
    println!("nostr_pubkey={}", config.nostr.public_key);
    Ok(())
}

fn default_config_path() -> PathBuf {
    if let Some(mut dir) = dirs::config_dir() {
        dir.push("nostr-vpn");
        dir.push("config.toml");
        return dir;
    }

    let mut fallback = PathBuf::from(".");
    fallback.push("nostr-vpn.toml");
    fallback
}

fn load_or_default_config(path: &Path) -> Result<AppConfig> {
    if path.exists() {
        return AppConfig::load(path);
    }

    let config = AppConfig::generated();
    config.save(path)?;
    Ok(config)
}

fn apply_participants_override(config: &mut AppConfig, participants: Vec<String>) -> Result<()> {
    if participants.is_empty() {
        return Ok(());
    }

    let mut normalized = participants
        .iter()
        .map(|participant| normalize_nostr_pubkey(participant))
        .collect::<Result<Vec<_>>>()?;

    normalized.sort();
    normalized.dedup();
    config.participants = normalized;

    if config.network_id.trim().is_empty() {
        config.network_id = derive_network_id_from_participants(&config.participants);
    }

    Ok(())
}

fn resolve_relays(cli_relays: &[String], config: &AppConfig) -> Vec<String> {
    if !cli_relays.is_empty() {
        return cli_relays.to_vec();
    }

    if !config.nostr.relays.is_empty() {
        return config.nostr.relays.clone();
    }

    DEFAULT_RELAYS
        .iter()
        .map(|relay| (*relay).to_string())
        .collect()
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
