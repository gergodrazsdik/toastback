#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Checks that every tracked source file (Rust and shell scripts) starts with
# `SPDX-License-Identifier: GPL-3.0-or-later` as a comment, on the first line
# or right after a shebang (CONTRIBUTING.md, "What goes in the repo").
#
# Usage: scripts/check-spdx.sh [<file>...]   (default: every tracked source file)
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

want=GPL-3.0-or-later
pattern='\.(rs|sh)$'
if [[ $# -gt 0 ]]; then
  files=("$@")
else
  mapfile -t files < <(git ls-files)
fi

failed=0
checked=0
for file in "${files[@]}"; do
  [[ $file =~ $pattern ]] || continue
  [[ -f $file ]] || continue
  head_lines=$(head -n 2 "$file")
  first=${head_lines%%$'\n'*}
  if [[ $first == '#!'* ]]; then
    line=${head_lines#*$'\n'}
  else
    line=$first
  fi
  checked=$((checked + 1))
  if [[ $line != *"SPDX-License-Identifier: $want"* ]]; then
    echo "error: $file: the first line (after a shebang) must carry SPDX-License-Identifier: $want" >&2
    failed=1
  fi
done

if [[ $failed -eq 0 && $# -eq 0 ]]; then
  echo "ok: $checked source files carry SPDX-License-Identifier: $want"
fi
exit "$failed"
