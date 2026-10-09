#!/usr/bin/env bash
# Rehearse the whole demo path against a live Cloud, in one command.
#
#   scripts/demo/rehearse.sh            # start Cloud if needed, run both rehearsals
#   scripts/demo/rehearse.sh --fresh    # drop the demo database first, so the
#                                       # console starts empty for the audience
#
# Starts the Cloud demo stack (scripts/demo-assessment.sh in the Cloud checkout,
# which needs Docker Desktop and Ollama) when nothing answers on 8787, waits
# for it, then runs the two ignored rehearsal tests: the function-level one
# and the one through the real Tauri commands. Each prints one line per demo
# step. Eight numbered lines and "test result: ok" twice means the demo works
# on this machine right now.
set -euo pipefail
app_root=$(cd "$(dirname "$0")/../.." && pwd)
cloud_root="${ALEXANDRIA_CLOUD_CHECKOUT:-$(dirname "$app_root")/alexandria-cloud-assessment-demo}"
cloud="${ALEXANDRIA_DEMO_CLOUD:-http://127.0.0.1:8787}"
fresh=0
for arg in "$@"; do
  case "$arg" in
    --fresh) fresh=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

if [ "$fresh" = 1 ]; then
  if [ -f "$cloud_root/.demo/server.pid" ]; then
    kill "$(cat "$cloud_root/.demo/server.pid")" 2>/dev/null || true
  fi
  pkill -f 'target/debug/server' 2>/dev/null || true
  docker rm -f alexandria-assessment-demo-db >/dev/null 2>&1 || true
  echo "demo database dropped"
fi

if ! curl --silent --fail --max-time 2 "$cloud/healthz" >/dev/null; then
  [ -x "$cloud_root/scripts/demo-assessment.sh" ] || { echo "Cloud checkout not found at $cloud_root (set ALEXANDRIA_CLOUD_CHECKOUT)" >&2; exit 1; }
  mkdir -p "$cloud_root/.demo"
  echo "starting Cloud from $cloud_root (log: $cloud_root/.demo/server.log)"
  (cd "$cloud_root" && nohup scripts/demo-assessment.sh > .demo/server.log 2>&1 & echo $! > .demo/server.pid)
  for _ in $(seq 1 180); do
    if curl --silent --fail --max-time 2 "$cloud/healthz" >/dev/null; then break; fi
    sleep 2
  done
  curl --silent --fail --max-time 2 "$cloud/healthz" >/dev/null || { echo "Cloud did not come up; see $cloud_root/.demo/server.log" >&2; exit 1; }
fi
echo "Cloud is up at $cloud"

cd "$app_root"
ALEXANDRIA_DEMO_CLOUD="$cloud" cargo test --manifest-path src-tauri/Cargo.toml --lib live_demo -- --ignored --nocapture --test-threads=1 2>&1 \
  | grep -E '^[0-9]\. |^test |test result|panicked|failed:'
echo
echo "Console: $cloud   (press Continue to sign in as the demo organisation)"
