use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use boringtun::device::{DeviceConfig, DeviceHandle};
use clap::{Args, Parser, Subcommand};
use hex::encode as encode_hex;
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
    /// Bring up a boringtun interface (Linux) for e2e testing.
    TunnelUp(TunnelUpArgs),
}

#[derive(Debug, Args)]
struct TunnelUpArgs {
    #[arg(long)]
    iface: String,
    #[arg(long)]
    private_key: String,
    #[arg(long)]
    listen_port: u16,
    #[arg(long)]
    address: String,
    #[arg(long)]
    peer_public_key: String,
    #[arg(long)]
    peer_endpoint: String,
    #[arg(long)]
    peer_allowed_ip: String,
    #[arg(long, default_value_t = 5)]
    keepalive_secs: u16,
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
        Command::TunnelUp(args) => tunnel_up(&args)?,
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

fn tunnel_up(args: &TunnelUpArgs) -> Result<()> {
    if cfg!(not(target_os = "linux")) {
        return Err(anyhow!("tunnel-up is currently supported on Linux only"));
    }

    if args.iface.trim().is_empty() {
        return Err(anyhow!("--iface must not be empty"));
    }

    let private_key_hex = key_b64_to_hex(&args.private_key)?;
    let peer_public_key_hex = key_b64_to_hex(&args.peer_public_key)?;

    // Keep handle alive for process lifetime; dropping tears down the device.
    let _handle = DeviceHandle::new(
        &args.iface,
        DeviceConfig {
            n_threads: 2,
            use_connected_socket: true,
            #[cfg(target_os = "linux")]
            use_multi_queue: false,
            #[cfg(target_os = "linux")]
            uapi_fd: -1,
        },
    )
    .with_context(|| format!("failed to create boringtun interface {}", args.iface))?;

    let uapi_socket = format!("/var/run/wireguard/{}.sock", args.iface);
    wait_for_socket(&uapi_socket)?;

    wg_set(
        &uapi_socket,
        &format!(
            "private_key={private_key_hex}\nlisten_port={}",
            args.listen_port
        ),
    )?;
    wg_set(
        &uapi_socket,
        &format!(
            "public_key={peer_public_key_hex}\nendpoint={}\nreplace_allowed_ips=true\nallowed_ip={}\npersistent_keepalive_interval={}",
            args.peer_endpoint, args.peer_allowed_ip, args.keepalive_secs
        ),
    )?;

    run_checked(
        ProcessCommand::new("ip")
            .arg("address")
            .arg("add")
            .arg(&args.address)
            .arg("dev")
            .arg(&args.iface),
    )?;
    run_checked(
        ProcessCommand::new("ip")
            .arg("link")
            .arg("set")
            .arg("mtu")
            .arg("1380")
            .arg("up")
            .arg("dev")
            .arg(&args.iface),
    )?;
    run_checked(
        ProcessCommand::new("ip")
            .arg("route")
            .arg("replace")
            .arg(&args.peer_allowed_ip)
            .arg("dev")
            .arg(&args.iface),
    )?;

    println!(
        "boringtun interface {} up: {}, peer {} via {}",
        args.iface, args.address, args.peer_allowed_ip, args.peer_endpoint
    );

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn key_b64_to_hex(value: &str) -> Result<String> {
    let bytes = STANDARD
        .decode(value)
        .with_context(|| "invalid base64 key encoding")?;
    if bytes.len() != 32 {
        return Err(anyhow!("expected 32-byte key material"));
    }
    Ok(encode_hex(bytes))
}

fn wait_for_socket(path: &str) -> Result<()> {
    for _ in 0..50 {
        if fs::metadata(path).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(anyhow!("timed out waiting for uapi socket at {path}"))
}

fn wg_set(socket_path: &str, body: &str) -> Result<()> {
    let mut socket =
        UnixStream::connect(socket_path).with_context(|| format!("connect {socket_path}"))?;
    write!(socket, "set=1\n{body}\n\n").context("failed to send uapi set")?;
    socket
        .shutdown(std::net::Shutdown::Write)
        .context("failed to close uapi write half")?;

    let mut response = String::new();
    socket
        .read_to_string(&mut response)
        .context("failed to read uapi response")?;

    if !response.contains("errno=0") {
        return Err(anyhow!("uapi set failed: {}", response.trim()));
    }

    Ok(())
}

fn run_checked(command: &mut ProcessCommand) -> Result<()> {
    let display = format!("{command:?}");
    let output = command
        .output()
        .with_context(|| format!("failed to execute {display}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow!(
            "command failed: {display}\nstdout: {}\nstderr: {}",
            stdout.trim(),
            stderr.trim()
        ));
    }

    Ok(())
}
