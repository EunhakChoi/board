#!/usr/bin/env sh
# Starts charet with Docker and prints the admin link.
#
#   ./start.sh              # docker compose up -d, wait, print the admin link
#   ./start.sh --build      # extra arguments go to `docker compose up -d`
#
# Right after install it also opens the link in the browser (set CHARET_NO_OPEN=1 to skip).
# Other compose files: COMPOSE_FILE=compose.yaml:compose.agent.yaml ./start.sh
set -eu
cd "$(dirname "$0")"

port=$(sed -n 's/^CHARET_PORT=//p' .env 2>/dev/null | tail -n 1)
port=${CHARET_PORT:-${port:-5280}}

docker compose up -d "$@"

printf 'Waiting for charet on port %s' "$port"
tries=0
until system=$(curl -fsS "http://127.0.0.1:$port/api/system" 2>/dev/null); do
  tries=$((tries + 1))
  if [ "$tries" -ge 120 ]; then
    echo
    echo "charet did not answer in 2 minutes. See: docker compose logs server" >&2
    exit 1
  fi
  printf '.'
  sleep 1
done
echo

token=$(docker compose exec -T server charet-server admin-token | tail -n 1)
link="http://localhost:$port/?t=$token"
echo
echo "charet is running."
echo "Admin link (open it once; the browser stays signed in, keep it to yourself):"
echo "  $link"
echo "Without the link, http://localhost:$port shows charet as a visitor."

# Open the browser only on the first run (the owner has not come in yet).
case "$system" in
  *'"firstRun":true'*)
    if [ -z "${CHARET_NO_OPEN:-}" ] && [ -z "${CI:-}" ] && [ -t 1 ]; then
      if command -v open >/dev/null 2>&1 && [ "$(uname)" = Darwin ]; then
        open "$link"
      elif command -v xdg-open >/dev/null 2>&1 && [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
        xdg-open "$link" >/dev/null 2>&1 || true
      fi
    fi
    ;;
esac
