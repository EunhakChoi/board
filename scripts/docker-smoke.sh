#!/usr/bin/env bash
# Installs charet with docker compose on spare ports and checks it end to end: the services answer,
# a project imports, its preview builds, every screen passes `check` and captures.
# KEEP=1 leaves the containers and volumes for a look afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."

export CHARET_PORT="${CHARET_PORT:-5380}" CHARET_PREVIEW_PORT="${CHARET_PREVIEW_PORT:-5381}"
export CHARET_UPDATE_CHECK=false
project=charet-smoke
compose() { docker compose -p "$project" "$@"; }
cli="$PWD/apps/charet-cli/src/main.ts"
charet() { CHARET_URL="http://127.0.0.1:$CHARET_PORT" node "$cli" "$@"; }
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
  charet doctor >/dev/null 2>&1 && break
  sleep 2
done
charet doctor

curl -sf "http://localhost:$CHARET_PORT/" | grep -q 'id="root"'
(cd templates/recommended && charet import --key smoke)
charet check -p smoke --theme both
charet capture -p smoke --theme both
charet ls -p smoke
echo "docker smoke: ok"
