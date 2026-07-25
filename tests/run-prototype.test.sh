#!/usr/bin/env sh
set -eu

case "${1:-}" in
  fake-docker)
    shift
    if [ "$*" = "compose config --environment" ]; then
      printf 'LIVEKIT_URL=%s\n' "${FAKE_LIVEKIT_URL:-}"
      printf 'HTTP_PORT=%s\n' "${HTTP_PORT:-}"
      exit 0
    fi

    printf '%s\n' "$*" >> "$DOCKER_CALLS"
    case " $* " in
      *" up "*) printf 'environment LAN_HOST=%s HTTP_PORT=%s\n' "${LAN_HOST:-}" "${HTTP_PORT:-}" >> "$DOCKER_CALLS" ;;
    esac
    exit 0
    ;;
  fake-hostname)
    exit 1
    ;;
esac

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_DIR=$(dirname -- "$SCRIPT_DIR")
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/run-prototype-test.XXXXXX")
trap 'rm -rf "$TMP_DIR"' EXIT HUP INT TERM

FAKE_BIN="$TMP_DIR/bin"
DOCKER_CALLS="$TMP_DIR/docker-calls"
OUTPUT="$TMP_DIR/output"
mkdir -p "$FAKE_BIN"
: > "$DOCKER_CALLS"

printf '%s\n' '#!/usr/bin/env sh' 'exec sh "$RUN_PROTOTYPE_TEST" fake-docker "$@"' > "$FAKE_BIN/docker"
printf '%s\n' '#!/usr/bin/env sh' 'exec sh "$RUN_PROTOTYPE_TEST" fake-hostname "$@"' > "$FAKE_BIN/hostname"
chmod +x "$FAKE_BIN/docker" "$FAKE_BIN/hostname"

export RUN_PROTOTYPE_TEST="$SCRIPT_DIR/run-prototype.test.sh"
export DOCKER_CALLS

if PATH="$FAKE_BIN:$PATH" FAKE_LIVEKIT_URL=wss://example.livekit.cloud HTTP_PORT=18080 LAN_IP= sh "$REPO_DIR/run-prototype.sh" > "$OUTPUT" 2>&1; then
  status=0
else
  status=$?
fi

if [ "$status" -ne 0 ]; then
  printf '%s\n' "Cloud startup failed without a LAN address:" >&2
  while IFS= read -r line; do
    printf '  %s\n' "$line" >&2
  done < "$OUTPUT"
  exit 1
fi

desktop_url_advertised=false
desktop_https_url_advertised=false
mobile_url_advertised=false
while IFS= read -r line; do
  case "$line" in
    *"Desktop: http://localhost:18080"*) desktop_url_advertised=true ;;
    *"Desktop HTTPS: https://localhost:8443"*) desktop_https_url_advertised=true ;;
    *"Mobile:"*) mobile_url_advertised=true ;;
  esac
done < "$OUTPUT"

if [ "$desktop_url_advertised" != true ] || [ "$desktop_https_url_advertised" != true ]; then
  printf '%s\n' "Cloud desktop startup did not advertise its configured HTTP and HTTPS URLs." >&2
  exit 1
fi

if [ "$mobile_url_advertised" = true ]; then
  printf '%s\n' "Cloud desktop startup advertised a loopback URL for mobile access." >&2
  exit 1
fi

cloud_gateway_configured=false
while IFS= read -r call; do
  case " $call " in
    *" --profile local "* | *" livekit "*)
      printf '%s\n' "Cloud startup enabled local LiveKit: $call" >&2
      exit 1
      ;;
    *" environment LAN_HOST=localhost HTTP_PORT=18080 "*)
      cloud_gateway_configured=true
      ;;
  esac
done < "$DOCKER_CALLS"

if [ "$cloud_gateway_configured" != true ]; then
  printf '%s\n' "Cloud desktop startup did not configure the localhost gateway." >&2
  exit 1
fi

if ! PATH="$FAKE_BIN:$PATH" FAKE_LIVEKIT_URL= LAN_IP=192.0.2.1 sh "$REPO_DIR/run-prototype.sh" > "$OUTPUT" 2>&1; then
  printf '%s\n' "Local startup failed with a LAN address:" >&2
  while IFS= read -r line; do
    printf '  %s\n' "$line" >&2
  done < "$OUTPUT"
  exit 1
fi

local_profile_enabled=false
default_desktop_url_advertised=false
while IFS= read -r call; do
  if [ "$call" = "compose --profile local up --build --detach" ]; then
    local_profile_enabled=true
  fi
done < "$DOCKER_CALLS"

while IFS= read -r line; do
  case "$line" in
    *"Desktop: http://localhost:8081"*) default_desktop_url_advertised=true ;;
  esac
done < "$OUTPUT"

if [ "$local_profile_enabled" != true ] || [ "$default_desktop_url_advertised" != true ]; then
  printf '%s\n' "Local startup did not enable LiveKit and advertise the default HTTP port." >&2
  exit 1
fi

printf '%s\n' "run-prototype profile tests passed"
