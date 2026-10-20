#!/usr/bin/env bash
# Installs charet with docker compose on spare ports and checks it end to end: the services answer,
# a project imports, its preview builds, every screen passes `check` and captures, and the web
# agent (scripted, no model) changes a branch from its own container.
# KEEP=1 leaves the containers and volumes for a look afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."

export CHARET_PORT="${CHARET_PORT:-5380}" CHARET_PREVIEW_PORT="${CHARET_PREVIEW_PORT:-5381}"
export CHARET_REMOTE_PORT="${CHARET_REMOTE_PORT:-5384}"
export CHARET_UPDATE_CHECK=false CHARET_AGENT_SCRIPT=true CHARET_AGENT_NETWORK=charet-smoke-agent-runs
project=charet-smoke
# `--env-file /dev/null`: this checkout's `.env` (e.g. a CHARET_DATA_DIR in use) stays out of it.
compose() {
  docker compose -p "$project" --env-file /dev/null -f compose.yaml -f compose.agent.yaml "$@"
}
cli="$PWD/apps/charet-cli/src/main.ts"
charet() {
  CHARET_URL="http://127.0.0.1:$CHARET_PORT" CHARET_TOKEN="${token:-}" CHARET_CONFIG_DIR="$config" \
    node "$cli" "$@"
}
config="$(mktemp -d)"
cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then compose logs --tail 80 || true; fi
  if [ -z "${KEEP:-}" ]; then compose down -v --remove-orphans >/dev/null 2>&1 || true; fi
  rm -rf "$config"
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
# No address grants anything: without a token the CLI is a visitor; the owner's token signs it in.
if charet whoami >/dev/null 2>&1; then echo "signed in without a token" >&2; exit 1; fi
token="$(compose exec -T server charet-server admin-token)"
charet whoami
(cd templates/recommended && charet import --key smoke)
charet check -p smoke --theme both
charet capture -p smoke --theme both
charet ls -p smoke

# The web agent: a run container checks out a new draft, writes a file and saves it.
CHARET_TOKEN="$token" node --input-type=module - "http://127.0.0.1:$CHARET_PORT" <<'JS'
const base = process.argv[2];
const headers = { "content-type": "application/json", authorization: `Bearer ${process.env.CHARET_TOKEN}` };
const api = async (method, path, body) => {
  const res = await fetch(base + path, { method, headers, body: body && JSON.stringify(body) });
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${await res.text()}`);
  return res.json();
};
const status = await api("GET", "/api/agent");
if (!status.enabled) throw new Error(`web agent off: ${status.reason}`);
const run = await api("POST", "/api/projects/smoke/agent-runs", { agent: "script", prompt: "write agent-smoke.txt: ok\nsay done" });
for (let i = 0; i < 120; i++) {
  const now = (await api("GET", `/api/projects/smoke/agent-runs?branch=${run.branch}`)).find((r) => r.id === run.id);
  if (["done", "failed", "canceled"].includes(now.state)) {
    const messages = await api("GET", `/api/projects/smoke/agent-runs/${run.id}/messages`);
    if (now.state !== "done") throw new Error(`run ${now.state}: ${now.error}\n${messages.map((m) => m.text).join("\n")}`);
    const branch = await api("GET", `/api/projects/smoke/branches/${run.branch}`);
    if (!branch.changes.some((c) => c.path === "agent-smoke.txt")) throw new Error("the run saved nothing");
    console.log(`web agent: branch #${run.branch} changed by a run container`);
    process.exit(0);
  }
  await new Promise((r) => setTimeout(r, 1000));
}
throw new Error("the run did not end");
JS
echo "docker smoke: ok"
