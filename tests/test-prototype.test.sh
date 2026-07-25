#!/usr/bin/env sh
set -eu

case "${1:-}" in
  fake-docker)
    shift
    if [ "$*" = "compose config --environment" ]; then
      printf 'LIVEKIT_URL=%s\n' "${LIVEKIT_URL:-}"
      printf 'LIVEKIT_API_KEY=%s\n' "${LIVEKIT_API_KEY:-}"
      printf 'LIVEKIT_API_SECRET=%s\n' "${LIVEKIT_API_SECRET:-}"
      exit 0
    fi
    exit 0
    ;;
  fake-run-prototype)
    printf 'LIVEKIT_URL=%s\n' "${LIVEKIT_URL:-}" > "$CAPTURED_ENVIRONMENT"
    printf 'LIVEKIT_API_KEY=%s\n' "${LIVEKIT_API_KEY:-}" >> "$CAPTURED_ENVIRONMENT"
    printf 'LIVEKIT_API_SECRET=%s\n' "${LIVEKIT_API_SECRET:-}" >> "$CAPTURED_ENVIRONMENT"
    printf 'LAN_IP=%s\n' "${LAN_IP:-}" >> "$CAPTURED_ENVIRONMENT"
    printf 'LAN_HOST=%s\n' "${LAN_HOST:-}" >> "$CAPTURED_ENVIRONMENT"
    exit 0
    ;;
esac

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_DIR=$(dirname -- "$SCRIPT_DIR")
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/test-prototype-test.XXXXXX")
trap 'rm -rf "$TMP_DIR"' EXIT HUP INT TERM

cp "$REPO_DIR/test-prototype.sh" "$TMP_DIR/test-prototype.sh"
printf '%s\n' '#!/usr/bin/env sh' 'exec sh "$TEST_PROTOTYPE_HELPER" fake-run-prototype' > "$TMP_DIR/run-prototype.sh"
printf '%s\n' '#!/usr/bin/env sh' 'exec sh "$TEST_PROTOTYPE_HELPER" fake-docker "$@"' > "$TMP_DIR/docker"
chmod +x "$TMP_DIR/run-prototype.sh" "$TMP_DIR/docker"

export TEST_PROTOTYPE_HELPER="$SCRIPT_DIR/test-prototype.test.sh"
export CAPTURED_ENVIRONMENT="$TMP_DIR/environment"
PATH="$TMP_DIR:$PATH"
export PATH

LIVEKIT_URL=wss://example.livekit.cloud \
LIVEKIT_API_KEY=cloud-key \
LIVEKIT_API_SECRET=cloud-secret \
  sh "$TMP_DIR/test-prototype.sh" >/dev/null

expected_local='LIVEKIT_URL=
LIVEKIT_API_KEY=devkey
LIVEKIT_API_SECRET=secret
LAN_IP=127.0.0.1
LAN_HOST=localhost'
captured_environment=$(while IFS= read -r line; do printf '%s\n' "$line"; done < "$CAPTURED_ENVIRONMENT")
if [ "$captured_environment" != "$expected_local" ]; then
  printf '%s\n' "Default compatibility mode did not force local configuration." >&2
  exit 1
fi

LIVEKIT_URL=wss://example.livekit.cloud \
LIVEKIT_API_KEY=cloud-key \
LIVEKIT_API_SECRET=cloud-secret \
  sh "$TMP_DIR/test-prototype.sh" cloud >/dev/null

expected_cloud='LIVEKIT_URL=wss://example.livekit.cloud
LIVEKIT_API_KEY=cloud-key
LIVEKIT_API_SECRET=cloud-secret
LAN_IP=
LAN_HOST='
captured_environment=$(while IFS= read -r line; do printf '%s\n' "$line"; done < "$CAPTURED_ENVIRONMENT")
if [ "$captured_environment" != "$expected_cloud" ]; then
  printf '%s\n' "Cloud compatibility mode did not preserve Cloud configuration." >&2
  exit 1
fi

if LIVEKIT_URL= LIVEKIT_API_KEY= LIVEKIT_API_SECRET= \
  sh "$TMP_DIR/test-prototype.sh" cloud >/dev/null 2>&1; then
  printf '%s\n' "Cloud compatibility mode accepted missing credentials." >&2
  exit 1
fi

printf '%s\n' "test-prototype mode tests passed"
