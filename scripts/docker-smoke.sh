#!/usr/bin/env bash
# Installs board with docker compose on spare ports and checks it end to end: the services answer,
# a project imports, its preview builds, every screen passes `check` and captures.
# KEEP=1 leaves the containers and volumes for a look afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."

export BOARD_PORT="${BOARD_PORT:-5380}" BOARD_PREVIEW_PORT="${BOARD_PREVIEW_PORT:-5381}"
export BOARD_UPDATE_CHECK=false
project=board-smoke
compose() { docker compose -p "$project" "$@"; }
cli="$PWD/apps/board-cli/src/main.ts"
board() { BOARD_URL="http://127.0.0.1:$BOARD_PORT" node "$cli" "$@"; }
cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then compose logs --tail 80 || true; fi
  if [ -z "${KEEP:-}" ]; then compose down -v --remove-orphans >/dev/null 2>&1 || true; fi
  exit "$status"
}
trap cleanup EXIT

# A fresh install: no volumes left from an earlier run.
compose down -v --remove-orphans >/dev/null 2>&1 || true
compose up -d --build
for _ in $(seq 60); do
  board doctor >/dev/null 2>&1 && break
  sleep 2
done
board doctor

curl -sf "http://localhost:$BOARD_PORT/" | grep -q 'id="root"'
(cd templates/recommended && board import --key smoke)
board check -p smoke --theme both
board capture -p smoke --theme both
board ls -p smoke
echo "docker smoke: ok"
