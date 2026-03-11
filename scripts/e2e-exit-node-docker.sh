#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
COMPOSE=(docker compose -f "$ROOT_DIR/docker-compose.nat-e2e.yml")

RELAY_URL="ws://10.254.241.2:8080"
REFLECTOR_ADDR="10.254.241.3:3478"
CONFIG_PATH="/root/.config/nvpn/config.toml"
EXIT_PUBLIC_IP="10.254.241.10"

cleanup() {
  "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
}

dump_debug() {
  set +e
  echo "exit-node docker e2e failed, collecting debug output..."
  "${COMPOSE[@]}" ps || true
  for service in relay reflector nat-a nat-b node-a node-b; do
    echo "--- logs: $service ---"
    "${COMPOSE[@]}" logs --no-color --tail 120 "$service" || true
  done
  for node in node-a node-b; do
    echo "--- $node status ---"
    "${COMPOSE[@]}" exec -T "$node" nvpn status --json --discover-secs 0 || true
    echo "--- $node daemon.state.json ---"
    "${COMPOSE[@]}" exec -T "$node" sh -lc "cat /root/.config/nvpn/daemon.state.json 2>/dev/null || true" || true
    echo "--- $node daemon.log ---"
    "${COMPOSE[@]}" exec -T "$node" sh -lc "tail -n 200 /root/.config/nvpn/daemon.log 2>/dev/null || true" || true
    echo "--- $node routes ---"
    "${COMPOSE[@]}" exec -T "$node" sh -lc "ip route || true" || true
    echo "--- $node utun100 ---"
    "${COMPOSE[@]}" exec -T "$node" sh -lc "ip addr show utun100 || true" || true
    echo "--- $node iptables ---"
    "${COMPOSE[@]}" exec -T "$node" sh -lc "iptables -S || true; iptables -t nat -S || true" || true
  done
}

on_exit() {
  local exit_code=$?
  if [[ $exit_code -ne 0 ]]; then
    dump_debug
  fi
  cleanup
  exit "$exit_code"
}
trap on_exit EXIT

compact_json() {
  tr -d '\n\r\t '
}

peer_tunnel_ip_from_status() {
  grep -o '"tunnel_ip":"10\.44\.0\.[0-9]\+/32"' | tail -n1 | cut -d '"' -f4 | cut -d/ -f1
}

peer_announced_endpoint_from_status() {
  grep -o '"endpoint":"[^"]*"' | tail -n1 | cut -d '"' -f4
}

json_public_endpoint() {
  grep -o '"public_endpoint":"[^"]*"' | tail -n1 | cut -d '"' -f4
}

wait_for_service() {
  local service="$1"
  local container_id=""
  for _ in $(seq 1 30); do
    container_id="$("${COMPOSE[@]}" ps -q "$service" 2>/dev/null || true)"
    if [[ -n "$container_id" ]] \
      && [[ "$(docker inspect -f '{{.State.Running}}' "$container_id" 2>/dev/null || true)" == "true" ]]; then
      return 0
    fi
    sleep 1
  done

  echo "exit-node docker e2e failed: service '$service' did not reach running state" >&2
  exit 1
}

ping_until_success() {
  local node="$1"
  local target="$2"
  local log_path="$3"
  for _ in $(seq 1 5); do
    if "${COMPOSE[@]}" exec -T "$node" ping -c 3 -W 2 "$target" >"$log_path"; then
      return 0
    fi
    sleep 2
  done

  return 1
}

cleanup

"${COMPOSE[@]}" build >/dev/null
"${COMPOSE[@]}" up -d relay reflector nat-a nat-b >/dev/null

for service in relay reflector nat-a nat-b; do
  wait_for_service "$service"
done

"${COMPOSE[@]}" up -d node-a node-b >/dev/null

for service in node-a node-b; do
  wait_for_service "$service"
done

"${COMPOSE[@]}" exec -T node-a sh -lc \
  "ip route del default >/dev/null 2>&1 || true; ip route add default via 198.19.241.2 dev eth0"
"${COMPOSE[@]}" exec -T node-b sh -lc \
  "ip route del default >/dev/null 2>&1 || true; ip route add default via 198.19.242.2 dev eth0"

for node in node-a node-b; do
  "${COMPOSE[@]}" exec -T "$node" nvpn init --force >/dev/null
done

ALICE_NPUB="$("${COMPOSE[@]}" exec -T node-a sh -lc \
  "grep -m1 '^public_key' '$CONFIG_PATH' | cut -d '\"' -f 2" | tr -d '\r')"
BOB_NPUB="$("${COMPOSE[@]}" exec -T node-b sh -lc \
  "grep -m1 '^public_key' '$CONFIG_PATH' | cut -d '\"' -f 2" | tr -d '\r')"

if [[ -z "$ALICE_NPUB" || -z "$BOB_NPUB" ]]; then
  echo "exit-node docker e2e failed: unable to resolve node npubs" >&2
  exit 1
fi

"${COMPOSE[@]}" exec -T node-a nvpn set \
  --participant "$BOB_NPUB" \
  --relay "$RELAY_URL" \
  --auto-disconnect-relays-when-mesh-ready false \
  --advertise-exit-node >/dev/null
"${COMPOSE[@]}" exec -T node-b nvpn set \
  --participant "$ALICE_NPUB" \
  --relay "$RELAY_URL" \
  --auto-disconnect-relays-when-mesh-ready false \
  --exit-node "$ALICE_NPUB" >/dev/null

for node in node-a node-b; do
  "${COMPOSE[@]}" exec -T "$node" sh -lc \
    "sed -i 's|^reflectors = .*|reflectors = [\"$REFLECTOR_ADDR\"]|' '$CONFIG_PATH'"
  "${COMPOSE[@]}" exec -T "$node" sh -lc \
    "sed -i 's|^discovery_timeout_secs = .*|discovery_timeout_secs = 2|' '$CONFIG_PATH'"
done

"${COMPOSE[@]}" exec -T node-a nvpn start --daemon --connect >/dev/null
"${COMPOSE[@]}" exec -T node-b nvpn start --daemon --connect >/dev/null

ALICE_STATUS=""
BOB_STATUS=""
for _ in $(seq 1 80); do
  ALICE_STATUS="$("${COMPOSE[@]}" exec -T node-a nvpn status --json --discover-secs 0 | tr -d '\r')"
  BOB_STATUS="$("${COMPOSE[@]}" exec -T node-b nvpn status --json --discover-secs 0 | tr -d '\r')"
  ALICE_COMPACT="$(printf '%s' "$ALICE_STATUS" | compact_json)"
  BOB_COMPACT="$(printf '%s' "$BOB_STATUS" | compact_json)"
  ALICE_ANNOUNCED_ENDPOINT="$(printf '%s' "$ALICE_COMPACT" | peer_announced_endpoint_from_status)"
  BOB_ANNOUNCED_ENDPOINT="$(printf '%s' "$BOB_COMPACT" | peer_announced_endpoint_from_status)"
  BOB_TUNNEL_IP="$(printf '%s' "$ALICE_COMPACT" | peer_tunnel_ip_from_status)"
  ALICE_TUNNEL_IP="$(printf '%s' "$BOB_COMPACT" | peer_tunnel_ip_from_status)"

  if grep -q '"status_source":"daemon"' <<<"$ALICE_COMPACT" \
    && grep -q '"status_source":"daemon"' <<<"$BOB_COMPACT" \
    && grep -q '"running":true' <<<"$ALICE_COMPACT" \
    && grep -q '"running":true' <<<"$BOB_COMPACT" \
    && grep -q '"advertised_routes":\["0.0.0.0/0","::/0"\]' <<<"$BOB_COMPACT" \
    && grep -q '"reachable":true' <<<"$BOB_COMPACT" \
    && [[ "$ALICE_ANNOUNCED_ENDPOINT" == "10.254.241.11:51820" ]] \
    && [[ "$BOB_ANNOUNCED_ENDPOINT" == "$EXIT_PUBLIC_IP:51820" ]] \
    && [[ -n "$ALICE_TUNNEL_IP" ]] \
    && [[ -n "$BOB_TUNNEL_IP" ]]; then
    break
  fi
  sleep 1
done

printf 'ALICE STATUS\n%s\n' "$ALICE_STATUS"
printf 'BOB STATUS\n%s\n' "$BOB_STATUS"

ALICE_COMPACT="$(printf '%s' "$ALICE_STATUS" | compact_json)"
BOB_COMPACT="$(printf '%s' "$BOB_STATUS" | compact_json)"
grep -q '"status_source":"daemon"' <<<"$ALICE_COMPACT"
grep -q '"status_source":"daemon"' <<<"$BOB_COMPACT"
grep -q '"running":true' <<<"$ALICE_COMPACT"
grep -q '"running":true' <<<"$BOB_COMPACT"
grep -q '"advertised_routes":\["0.0.0.0/0","::/0"\]' <<<"$BOB_COMPACT"
grep -q '"reachable":true' <<<"$BOB_COMPACT"

BOB_TUNNEL_IP="$(printf '%s' "$ALICE_COMPACT" | peer_tunnel_ip_from_status)"
ALICE_TUNNEL_IP="$(printf '%s' "$BOB_COMPACT" | peer_tunnel_ip_from_status)"
if [[ -z "$ALICE_TUNNEL_IP" || -z "$BOB_TUNNEL_IP" ]]; then
  echo "exit-node docker e2e failed: unable to resolve peer tunnel IPs from status output" >&2
  exit 1
fi

PEER_ROUTE="$("${COMPOSE[@]}" exec -T node-b sh -lc "ip route get $EXIT_PUBLIC_IP | tr -d '\r'")"
REFLECTOR_ROUTE="$("${COMPOSE[@]}" exec -T node-b sh -lc "ip route get 10.254.241.3 | tr -d '\r'")"

if grep -q 'dev utun100' <<<"$PEER_ROUTE"; then
  echo "exit-node docker e2e failed: peer public endpoint route unexpectedly points into the tunnel" >&2
  echo "$PEER_ROUTE"
  exit 1
fi

if ! grep -q 'dev utun100' <<<"$REFLECTOR_ROUTE"; then
  echo "exit-node docker e2e failed: reflector route did not switch to the tunnel" >&2
  echo "$REFLECTOR_ROUTE"
  exit 1
fi

NAT_DISCOVER="$("${COMPOSE[@]}" exec -T node-b nvpn nat-discover --reflector "$REFLECTOR_ADDR" --listen-port 53000 --json | tr -d '\r')"
PUBLIC_ENDPOINT="$(printf '%s' "$NAT_DISCOVER" | compact_json | json_public_endpoint)"

if [[ "$PUBLIC_ENDPOINT" != "$EXIT_PUBLIC_IP:"* ]]; then
  echo "exit-node docker e2e failed: nat discovery did not egress through exit node ('$PUBLIC_ENDPOINT')" >&2
  exit 1
fi

echo "--- Peer endpoint route ---"
echo "$PEER_ROUTE"
echo "--- Reflector route ---"
echo "$REFLECTOR_ROUTE"
echo "--- Node B NAT discover ---"
echo "$NAT_DISCOVER"

echo "exit-node docker e2e passed: default-route traffic egressed through the selected exit node while the peer endpoint stayed outside the tunnel"
