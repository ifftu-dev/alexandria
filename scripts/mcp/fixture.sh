#!/usr/bin/env bash
# Build the fixture profile and serve it over the assistant broker.
#
#   scripts/mcp/fixture.sh            # serve until Ctrl-C
#   scripts/mcp/fixture.sh --clean    # remove what a previous run created
#
# Everything lives under /tmp/alexandria-mcp-fixture (override with
# ALEXANDRIA_FIXTURE_DIR): a throwaway profile database, the broker socket and
# one grant. A Unix socket path has a hard length limit, which is why this is
# not under target/. No real profile, MCP client configuration or environment
# file is read or written.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
fixture="${ALEXANDRIA_FIXTURE_DIR:-/tmp/alexandria-mcp-fixture}"

if [[ "${1:-}" == "--clean" ]]; then
    rm -rf "$fixture"
    echo "removed $fixture"
    exit 0
fi

cargo +1.91.0 build -p alexandria-mcp
exec cargo +1.91.0 run -q -p alexandria-studio --example fixture_host -- "$fixture"
