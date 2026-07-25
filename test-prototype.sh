#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$SCRIPT_DIR"

docker compose down --remove-orphans
./run-prototype.sh
docker compose --profile test run --build --rm smoke
