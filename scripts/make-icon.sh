#!/usr/bin/env bash
# Render the placeholder icon: res/icon.svg -> res/icon.png (256 px) + res/icon.ico (multi-size).
# Needs resvg and ImageMagick (`magick`).
set -euo pipefail
cd "$(dirname "$0")/../res"
resvg -w 256 -h 256 icon.svg icon.png
magick icon.png -define icon:auto-resize=256,64,48,32,24,16 icon.ico
