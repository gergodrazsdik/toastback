#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Run a lint tool if it is installed; otherwise skip with a clear message.
# Usage: scripts/with-tool.sh <tool> [args...]
#
# Skipping is for local hooks and recipes only: CI sets TOASTBACK_REQUIRE_TOOLS=1,
# which turns a missing tool into a failure, so nothing passes unchecked there.
set -eu

tool=$1
shift

if command -v "$tool" >/dev/null 2>&1; then
  exec "$tool" "$@"
fi

if [ "${TOASTBACK_REQUIRE_TOOLS:-0}" = 1 ]; then
  echo "error: $tool is not installed (required). Run 'just tools' for install hints." >&2
  exit 127
fi

echo "SKIPPED: $tool is not installed, so this check did not run. Run 'just tools' for install hints." >&2
exit 0
