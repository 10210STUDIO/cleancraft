#!/bin/bash
# Render the brand SVGs to the PNGs the app and the installer use. Deterministic: the same
# SVG always gives the same pixels with the same resvg version.
#
#   assets/brand/AppIcon.svg          -> assets/AppIcon.png (1024 x 1024)
#   assets/brand/MenuBarTemplate.svg  -> assets/MenuBarTemplate.png (18 x 18)
#                                        assets/MenuBarTemplate@2x.png (36 x 36)
#
# Uses the resvg command line tool when installed (cargo install resvg), else the resvg
# Python binding (pip install resvg-py).
set -euo pipefail
cd "$(dirname "$0")/.."

render() { # svg out width
  if command -v resvg >/dev/null; then
    resvg --width "$3" --height "$3" "$1" "$2"
  else
    python3 - "$1" "$2" "$3" <<'PY'
import sys
import resvg_py
svg, out, w = sys.argv[1], sys.argv[2], int(sys.argv[3])
data = resvg_py.svg_to_bytes(svg_path=svg, width=w, height=w, skip_system_fonts=True)
open(out, "wb").write(bytes(data))
PY
  fi
  echo "rendered $2 ($3 px)"
}

render assets/brand/AppIcon.svg assets/AppIcon.png 1024
render assets/brand/MenuBarTemplate.svg assets/MenuBarTemplate.png 18
render assets/brand/MenuBarTemplate.svg assets/MenuBarTemplate@2x.png 36
