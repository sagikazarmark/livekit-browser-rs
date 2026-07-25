#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$SCRIPT_DIR"

MODE="${1:-local}"
if [ "$#" -gt 1 ]; then
  printf '%s\n' "Usage: ./test-prototype.sh [local|cloud]" >&2
  exit 2
fi

case "$MODE" in
  local)
    export LIVEKIT_URL=
    export LIVEKIT_API_KEY=devkey
    export LIVEKIT_API_SECRET=secret
    export LAN_IP="${LAN_IP:-127.0.0.1}"
    export LAN_HOST="${LAN_HOST:-localhost}"
    printf '%s\n' "Compatibility backend: local LiveKit"
    ;;
  cloud)
    COMPOSE_ENVIRONMENT=$(docker compose config --environment)
    for required_name in LIVEKIT_URL LIVEKIT_API_KEY LIVEKIT_API_SECRET; do
      required_value=$(
        printf '%s\n' "$COMPOSE_ENVIRONMENT" |
          while IFS='=' read -r name value; do
            if [ "$name" = "$required_name" ]; then
              printf '%s' "$value"
              break
            fi
          done
      )
      if [ -z "$required_value" ]; then
        printf '%s\n' "$required_name is required for Cloud compatibility tests." >&2
        exit 1
      fi
    done
    printf '%s\n' "Compatibility backend: LiveKit Cloud"
    ;;
  *)
    printf '%s\n' "Usage: ./test-prototype.sh [local|cloud]" >&2
    exit 2
    ;;
esac

docker compose down --remove-orphans
./run-prototype.sh
docker compose --profile test run --build --rm smoke
