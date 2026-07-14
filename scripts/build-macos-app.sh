#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Swanium"
BUNDLE_ID="bubio.swanium"
VERSION="$(sed -n '/^\[workspace\.package\]/,/^\[/ { s/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p; }' Cargo.toml | head -n 1)"
MIN_MACOS_VERSION="13.5"
DESCRIPTION="A cross-platform WonderSwan / WonderSwan Color emulator written in Rust."
COPYRIGHT="Copyright © 2026 Bubio"
PACKAGE="frontend"
BINARY_NAME="frontend"
ICON_SOURCE="assets/icons/AppIcon.png"
TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin")
ARCHS=("arm64" "x86_64")
PROFILE="release"
PROFILE_FLAG="--release"
TARGET_DIR="target"
APP_DIR="${TARGET_DIR}/${PROFILE}/${APP_NAME}.app"
ZIP_PATH="${TARGET_DIR}/${PROFILE}/${APP_NAME}-${VERSION}-macos-universal.zip"
ASSET_CATALOG_DIR="${TARGET_DIR}/${PROFILE}/${APP_NAME}.xcassets"
APPICONSET_DIR="${ASSET_CATALOG_DIR}/AppIcon.appiconset"
ACTOOL_PARTIAL_PLIST="${TARGET_DIR}/${PROFILE}/${APP_NAME}-asset-info.plist"
ACTOOL_LOG="${TARGET_DIR}/${PROFILE}/${APP_NAME}-actool.log"
ICON_NORMALIZED="${TARGET_DIR}/${PROFILE}/${APP_NAME}-AppIcon-1023.png"

usage() {
  cat <<EOF
Usage: $0 [--app-dir PATH] [--zip-path PATH] [--icon-source PATH]

Builds an unsigned universal macOS App Bundle for ${APP_NAME}.

Defaults:
  App bundle: ${APP_DIR}
  Zip file:   ${ZIP_PATH}
  Icon PNG:   ${ICON_SOURCE}
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --app-dir)
      APP_DIR="$2"
      shift 2
      ;;
    --zip-path)
      ZIP_PATH="$2"
      shift 2
      ;;
    --icon-source)
      ICON_SOURCE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: macOS App Bundle builds must run on macOS" >&2
  exit 1
fi

if [[ -z "${VERSION}" ]]; then
  echo "error: workspace version not found in Cargo.toml" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is required" >&2
  exit 1
fi

if ! command -v lipo >/dev/null 2>&1; then
  echo "error: lipo is required; install Xcode command line tools" >&2
  exit 1
fi

if ! command -v xcrun >/dev/null 2>&1; then
  echo "error: xcrun is required; install Xcode command line tools" >&2
  exit 1
fi

if ! command -v sips >/dev/null 2>&1; then
  echo "error: sips is required" >&2
  exit 1
fi

if [[ ! -f "${ICON_SOURCE}" ]]; then
  echo "error: icon source not found: ${ICON_SOURCE}" >&2
  exit 1
fi

if [[ "${SKIP_RUSTUP_TARGET_ADD:-0}" != "1" ]]; then
  rustup target add "${TARGETS[@]}"
fi

export MACOSX_DEPLOYMENT_TARGET="${MIN_MACOS_VERSION}"

for target in "${TARGETS[@]}"; do
  cargo build -p "${PACKAGE}" "${PROFILE_FLAG}" --target "${target}"
done

rm -rf "${APP_DIR}"
mkdir -p "${APP_DIR}/Contents/MacOS" "${APP_DIR}/Contents/Resources"

lipo -create \
  "${TARGET_DIR}/${TARGETS[0]}/${PROFILE}/${BINARY_NAME}" \
  "${TARGET_DIR}/${TARGETS[1]}/${PROFILE}/${BINARY_NAME}" \
  -output "${APP_DIR}/Contents/MacOS/${APP_NAME}"
chmod +x "${APP_DIR}/Contents/MacOS/${APP_NAME}"

rm -rf "${ASSET_CATALOG_DIR}"
mkdir -p "${APPICONSET_DIR}"
sips -s format png -z 16 16 "${ICON_SOURCE}" --out "${APPICONSET_DIR}/icon_16.png" >/dev/null
sips -s format png -z 32 32 "${ICON_SOURCE}" --out "${APPICONSET_DIR}/icon_16@2x.png" >/dev/null
sips -s format png -z 32 32 "${ICON_SOURCE}" --out "${APPICONSET_DIR}/icon_32.png" >/dev/null
sips -s format png -z 64 64 "${ICON_SOURCE}" --out "${APPICONSET_DIR}/icon_32@2x.png" >/dev/null
sips -s format png -z 128 128 "${ICON_SOURCE}" --out "${APPICONSET_DIR}/icon_128.png" >/dev/null
sips -s format png -z 256 256 "${ICON_SOURCE}" --out "${APPICONSET_DIR}/icon_128@2x.png" >/dev/null
sips -s format png -z 256 256 "${ICON_SOURCE}" --out "${APPICONSET_DIR}/icon_256.png" >/dev/null
sips -s format png -z 512 512 "${ICON_SOURCE}" --out "${APPICONSET_DIR}/icon_256@2x.png" >/dev/null
sips -s format png -z 512 512 "${ICON_SOURCE}" --out "${APPICONSET_DIR}/icon_512.png" >/dev/null
sips -s format png -z 1023 1023 "${ICON_SOURCE}" --out "${ICON_NORMALIZED}" >/dev/null
sips -s format png -z 1024 1024 "${ICON_NORMALIZED}" --out "${APPICONSET_DIR}/icon_512@2x.png" >/dev/null
cat > "${APPICONSET_DIR}/Contents.json" <<EOF
{
  "images": [
    { "idiom": "mac", "size": "16x16", "scale": "1x", "filename": "icon_16.png" },
    { "idiom": "mac", "size": "16x16", "scale": "2x", "filename": "icon_16@2x.png" },
    { "idiom": "mac", "size": "32x32", "scale": "1x", "filename": "icon_32.png" },
    { "idiom": "mac", "size": "32x32", "scale": "2x", "filename": "icon_32@2x.png" },
    { "idiom": "mac", "size": "128x128", "scale": "1x", "filename": "icon_128.png" },
    { "idiom": "mac", "size": "128x128", "scale": "2x", "filename": "icon_128@2x.png" },
    { "idiom": "mac", "size": "256x256", "scale": "1x", "filename": "icon_256.png" },
    { "idiom": "mac", "size": "256x256", "scale": "2x", "filename": "icon_256@2x.png" },
    { "idiom": "mac", "size": "512x512", "scale": "1x", "filename": "icon_512.png" },
    { "idiom": "mac", "size": "512x512", "scale": "2x", "filename": "icon_512@2x.png" }
  ],
  "info": { "author": "xcode", "version": 1 }
}
EOF
if ! xcrun actool "${ASSET_CATALOG_DIR}" \
  --compile "${APP_DIR}/Contents/Resources" \
  --platform macosx \
  --minimum-deployment-target "${MIN_MACOS_VERSION}" \
  --app-icon AppIcon \
  --output-partial-info-plist "${ACTOOL_PARTIAL_PLIST}" >"${ACTOOL_LOG}" 2>&1; then
  cat "${ACTOOL_LOG}" >&2
  exit 1
fi

cat > "${APP_DIR}/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
 "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleExecutable</key>
  <string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleIconName</key>
  <string>AppIcon</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleGetInfoString</key>
  <string>${DESCRIPTION}</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleSupportedPlatforms</key>
  <array>
    <string>MacOSX</string>
  </array>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key>
  <string>${MIN_MACOS_VERSION}</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSHumanReadableCopyright</key>
  <string>${COPYRIGHT}</string>
</dict>
</plist>
EOF

plutil -lint "${APP_DIR}/Contents/Info.plist"
file "${APP_DIR}/Contents/MacOS/${APP_NAME}"
lipo "${APP_DIR}/Contents/MacOS/${APP_NAME}" -verify_arch "${ARCHS[@]}"

rm -f "${ZIP_PATH}"
mkdir -p "$(dirname "${ZIP_PATH}")"
ditto -c -k --keepParent "${APP_DIR}" "${ZIP_PATH}"

echo "Built ${APP_DIR}"
echo "Packaged ${ZIP_PATH}"
