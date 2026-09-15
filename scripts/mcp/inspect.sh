#!/usr/bin/env bash
set -euo pipefail
studio_root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$studio_root"
cargo +1.91.0 build -p alexandria-mcp
exec npx --yes @modelcontextprotocol/inspector@2.6.0 "$studio_root/target/debug/alexandria-mcp"
