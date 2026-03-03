# nostr-vpn

`nostr-vpn` is a Tailscale-style control plane on Nostr with:

- `nostr-vpn-cli`: multiplatform CLI
- `nostr-vpn-gui`: multiplatform desktop GUI (macOS-first UX)
- `nostr-vpn-core`: shared core logic (boringtun keying + Nostr signaling)

## What is implemented

- Boringtun key generation and handshake simulation (`boringtun::noise::Tunn`)
- Nostr relay signaling client and message envelope
- Network membership based on participant Nostr pubkeys (npub/hex allowlist)
- Deterministic network IDs derived from participant pubkeys
- Automatic key generation for both WireGuard and Nostr identities
- GUI with topbar controls + settings dialog
- E2E signaling test against a local websocket relay (no external infra)

## Default relays

Defaults match `~/src/hashtree`:

- `wss://temp.iris.to`
- `wss://relay.damus.io`
- `wss://nos.lol`
- `wss://relay.primal.net`
- `wss://offchain.pub`

## Quickstart

### 1. Build and test

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

### 2. Create config (auto-keygen)

```bash
nostr-vpn init \
  --participant npub1...alice \
  --participant npub1...bob
```

This writes config to `~/.config/nostr-vpn/config.toml`.

### 3. Announce your node

```bash
nostr-vpn announce --endpoint 203.0.113.10:51820 --tunnel-ip 10.44.0.2/32
```

### 4. Listen for peers

```bash
nostr-vpn listen
```

### 5. Render WireGuard config

```bash
nostr-vpn render-wg \
  --peer "<wg-pubkey>,10.44.0.3/32,198.51.100.20:51820"
```

### 6. Start GUI

```bash
cargo run -p nostr-vpn-gui
```

Use the topbar controls (`Menu`, `Connect`, `Announce`, `Settings`).

## Docker e2e

Run a real cross-container signaling check (relay + 2 nodes):

```bash
./scripts/e2e-docker.sh
```

What it validates:

- Relay container accepts Nostr websocket connections
- Alice and Bob containers exchange announcements over Nostr
- Both containers bring up boringtun interfaces (`tunnel-up`)
- Tunnel data plane works (`ping` over `10.44.0.1 <-> 10.44.0.2`)

## Notes

- This is currently a control-plane MVP with config rendering for WireGuard peers.
- Participant pubkey lists are normalized to hex and used as a sender allowlist.
- If participants are set, network ID is derived automatically from those pubkeys.
