#!/usr/bin/env sh
set -eu

if ! command -v docker >/dev/null 2>&1; then
  printf '%s\n' "Docker is required to run this prototype." >&2
  exit 1
fi

COMPOSE_ENVIRONMENT=$(docker compose config --environment)
COMPOSE_LIVEKIT_URL=$(
  printf '%s\n' "$COMPOSE_ENVIRONMENT" |
    while IFS='=' read -r name value; do
      if [ "$name" = "LIVEKIT_URL" ]; then
        printf '%s' "$value"
        break
      fi
    done
)
HTTP_PORT=$(
  printf '%s\n' "$COMPOSE_ENVIRONMENT" |
    while IFS='=' read -r name value; do
      if [ "$name" = "HTTP_PORT" ]; then
        printf '%s' "$value"
        break
      fi
    done
)
HTTP_PORT="${HTTP_PORT:-8081}"
export HTTP_PORT
MOBILE_URL_AVAILABLE=true

if [ -z "$COMPOSE_LIVEKIT_URL" ]; then
  if [ -z "${LAN_IP:-}" ]; then
    set -- $(hostname -I 2>/dev/null || true)
    LAN_IP="${1:-}"
  fi

  if [ -z "$LAN_IP" ]; then
    printf '%s\n' "Could not detect a LAN address. Run with LAN_IP=192.168.x.x ./run-prototype.sh" >&2
    exit 1
  fi
else
  if [ -z "${LAN_IP:-}" ] && [ -z "${LAN_HOST:-}" ]; then
    MOBILE_URL_AVAILABLE=false
    LAN_HOST=localhost
  fi
  LAN_IP="${LAN_IP:-127.0.0.1}"
fi

export LAN_IP
LAN_HOST="${LAN_HOST:-${LAN_IP}.nip.io}"
export LAN_HOST
mkdir -p .prototype-runtime

if [ -n "$COMPOSE_LIVEKIT_URL" ]; then
  docker compose up --build --detach
else
  docker compose --profile local up --build --detach
fi

attempt=0
until docker compose exec -T gateway test -f /data/caddy/pki/authorities/local/root.crt 2>/dev/null; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 30 ]; then
    printf '%s\n' "The stack started, but the local HTTPS root certificate was not ready." >&2
    exit 1
  fi
  sleep 1
done

docker compose cp gateway:/data/caddy/pki/authorities/local/root.crt .prototype-runtime/caddy-local-root.crt >/dev/null

printf '\n%s\n' "LiveKit + Dioxus prototype is running."
printf '  Desktop: %s\n' "http://localhost:${HTTP_PORT}"
if [ "$MOBILE_URL_AVAILABLE" = true ]; then
  printf '  Mobile:  %s\n' "https://${LAN_HOST}:8443"
  printf '  Mobile CA certificate: %s\n\n' ".prototype-runtime/caddy-local-root.crt"
  printf '%s\n' "Trust the CA certificate on the phone before opening the mobile URL. See README.md."
else
  printf '  Desktop HTTPS: %s\n' "https://localhost:8443"
  printf '  Local CA certificate: %s\n\n' ".prototype-runtime/caddy-local-root.crt"
  printf '%s\n' "Trust the CA certificate to use desktop HTTPS, or set LAN_IP to enable mobile access."
fi
printf '%s\n' "Follow logs with: docker compose logs -f"
