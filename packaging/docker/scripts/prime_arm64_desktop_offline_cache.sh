#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
EXPORT_OFFLINE_BUNDLE="${EXPORT_OFFLINE_BUNDLE:-1}"

# Prime every cache with one online build so later offline rebuilds can reuse them.
FORCE_NPM_INSTALL=1 SKIP_NPM_INSTALL=0 WUNDER_BUILD_OFFLINE=0 WUNDER_BUILD_FRONTEND=1 bash "${ROOT_DIR}/packaging/docker/scripts/build_arm64_desktop_with_python.sh"

if [ "${EXPORT_OFFLINE_BUNDLE}" = "1" ]; then
  bash "${ROOT_DIR}/packaging/docker/scripts/export_arm64_desktop_offline_bundle.sh"
fi

echo "Offline priming finished."
echo "Recommended offline rebuild command:"
echo "  WUNDER_BUILD_OFFLINE=1 SKIP_NPM_INSTALL=1 WUNDER_BUILD_FRONTEND=1 bash packaging/docker/scripts/build_arm64_desktop_with_python.sh"
