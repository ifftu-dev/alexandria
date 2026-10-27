#!/usr/bin/env bash
# Drive the installed Mac app through the Accessibility API, for demo
# rehearsals and capture.
#
#   scripts/demo/ax.sh [--panel] dump               # list buttons, fields, links, text
#   scripts/demo/ax.sh [--panel] press LABEL [n]    # press the nth match (default 1)
#   scripts/demo/ax.sh [--panel] fill LABEL TEXT    # set a text field's value
#   scripts/demo/ax.sh [--panel] box LABEL [n]      # print x,y,w,h and role, for cliclick
#
# LABEL matches an element's title, description, placeholder, or a static
# text's value. Targets /Applications/Alexandria.app by default; --panel
# targets the side-by-side "Alexandria Panel.app" copy instead. Builds axtool
# from axtool.swift on first run (needs swiftc from the Xcode command line
# tools). The terminal running it needs Accessibility permission.
#
# Fill does not reach every Vue v-model (the unlock password, for one): use
# box, then cliclick to click, select all, delete and type. For a native
# <select>, box LABEL 2 finds its AXPopUpButton.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
tool="$here/axtool"
if [ ! -x "$tool" ] || [ "$here/axtool.swift" -nt "$tool" ]; then
  swiftc -O "$here/axtool.swift" -o "$tool"
fi

pat='/Applications/Alexandria.app/Contents/MacOS/alexandria-node'
if [ "${1:-}" = --panel ]; then pat='Alexandria Panel.app/Contents/MacOS/'; shift; fi
pid=$(pgrep -f "$pat" | head -1 || true)
[ -z "$pid" ] && { echo "app not running" >&2; exit 1; }
exec timeout 60 "$tool" "$pid" "$@"
