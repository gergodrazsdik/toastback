#!/usr/bin/env bash
# Stand-in "deploy" for examples/deploy.json: shows what a callback sees.
set -euo pipefail
echo "deploying build $(printf %s "$TOASTBACK_DATA" | sed -n 's/.*"build":\([0-9]*\).*/\1/p') with: $*"
echo "distro=$WSL_DISTRO_NAME user=$(id -un) cwd=$PWD"
echo "id=$TOASTBACK_ID action=$TOASTBACK_ACTION title=$TOASTBACK_TITLE"
echo "data=$TOASTBACK_DATA"
