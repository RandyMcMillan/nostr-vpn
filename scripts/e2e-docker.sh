#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
COMPOSE=(docker compose -f "$ROOT_DIR/docker-compose.e2e.yml")
LISTENER_NAME="nostr-vpn-bob-listener"

cleanup() {
  docker rm -f "$LISTENER_NAME" >/dev/null 2>&1 || true
  "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT

cleanup

"${COMPOSE[@]}" build >/dev/null
"${COMPOSE[@]}" up -d relay >/dev/null
sleep 2

"${COMPOSE[@]}" run -d --name "$LISTENER_NAME" node \
  listen \
  --network-id docker-e2e \
  --relay ws://relay:8080 \
  --limit 1 >/dev/null

sleep 2

"${COMPOSE[@]}" run --rm node \
  announce \
  --network-id docker-e2e \
  --relay ws://relay:8080 \
  --node-id alice-node \
  --endpoint alice:51820 \
  --tunnel-ip 10.44.0.2/32 \
  --public-key test-wg-public-key-alice >/dev/null

docker wait "$LISTENER_NAME" >/dev/null
LISTENER_LOGS="$(docker logs "$LISTENER_NAME" 2>&1 || true)"

echo "$LISTENER_LOGS"

if ! grep -Eq '"node_id"\s*:\s*"alice-node"' <<<"$LISTENER_LOGS"; then
  echo "docker e2e failed: bob did not receive alice announcement" >&2
  exit 1
fi

echo "docker e2e passed: bob received alice announcement"
