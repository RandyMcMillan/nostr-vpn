#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
WEB_DIST_DIR="$(cd "${PROJECT_DIR}/../../.." && pwd)/dist"
ASSETS_DIR="${PROJECT_DIR}/assets"
IOS_ICON_SOURCE_DIR="$(cd "${PROJECT_DIR}/../.." && pwd)/icons/ios"
IOS_APPICONSET_DIR="${PROJECT_DIR}/Assets.xcassets/AppIcon.appiconset"

if [[ ! -f "${WEB_DIST_DIR}/index.html" ]]; then
  echo "expected frontend build output at ${WEB_DIST_DIR}/index.html" >&2
  exit 1
fi

mkdir -p "${ASSETS_DIR}"
rsync -a --delete "${WEB_DIST_DIR}/" "${ASSETS_DIR}/"

if [[ ! -d "${IOS_ICON_SOURCE_DIR}" ]]; then
  echo "expected iOS icon sources at ${IOS_ICON_SOURCE_DIR}" >&2
  exit 1
fi

if [[ ! -f "${IOS_APPICONSET_DIR}/Contents.json" ]]; then
  echo "expected iOS AppIcon catalog at ${IOS_APPICONSET_DIR}" >&2
  exit 1
fi

find "${IOS_APPICONSET_DIR}" -maxdepth 1 -type f -name '*.png' -delete
rsync -a "${IOS_ICON_SOURCE_DIR}/" "${IOS_APPICONSET_DIR}/"
