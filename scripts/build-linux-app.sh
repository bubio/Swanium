#!/usr/bin/env bash
set -euo pipefail

PACKAGE="frontend"
DIST_DIR="dist"
ARCHITECTURE=""
DEB_DIR="${DIST_DIR}/deb"
RPM_DIR="${DIST_DIR}/rpm"
VERSION="$(sed -n '/^\[workspace\.package\]/,/^\[/ { s/^[[:space:]]*version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p; }' Cargo.toml | head -n 1)"

usage() {
  cat <<EOF
Usage: $0 --architecture ARCH [--dist-dir PATH]

Builds Linux release packages (.deb and .rpm).

Defaults:
  Dist dir: ${DIST_DIR}

Architectures:
  x64, arm64
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dist-dir)
      DIST_DIR="$2"
      DEB_DIR="${DIST_DIR}/deb"
      RPM_DIR="${DIST_DIR}/rpm"
      shift 2
      ;;
    --architecture)
      ARCHITECTURE="$2"
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

if [[ "${ARCHITECTURE}" != "x64" && "${ARCHITECTURE}" != "arm64" ]]; then
  echo "error: --architecture must be x64 or arm64" >&2
  exit 2
fi

if [[ -z "${VERSION}" ]]; then
  echo "error: workspace version not found in Cargo.toml" >&2
  exit 1
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "error: Linux packaging must run on Linux" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is required" >&2
  exit 1
fi

if ! cargo deb --help >/dev/null 2>&1; then
  echo "error: cargo-deb is required (cargo install cargo-deb --locked)" >&2
  exit 1
fi

if ! cargo generate-rpm --help >/dev/null 2>&1; then
  echo "error: cargo-generate-rpm is required (cargo install cargo-generate-rpm --locked)" >&2
  exit 1
fi

cargo build -p "${PACKAGE}" --release
cargo deb -p "${PACKAGE}" --no-build
cargo generate-rpm -p "crates/${PACKAGE}"

rm -rf "${DEB_DIR}" "${RPM_DIR}"
mkdir -p "${DEB_DIR}" "${RPM_DIR}"

shopt -s nullglob
deb_packages=(target/debian/swanium_"${VERSION}"-*.deb)
rpm_packages=(target/generate-rpm/swanium-"${VERSION}"-*.rpm)
if [[ "${#deb_packages[@]}" -ne 1 ]]; then
  echo "error: expected exactly one DEB package, found ${#deb_packages[@]}" >&2
  exit 1
fi
if [[ "${#rpm_packages[@]}" -ne 1 ]]; then
  echo "error: expected exactly one RPM package, found ${#rpm_packages[@]}" >&2
  exit 1
fi

cp "${deb_packages[0]}" "${DEB_DIR}/Swanium-${VERSION}-linux-${ARCHITECTURE}.deb"
cp "${rpm_packages[0]}" "${RPM_DIR}/Swanium-${VERSION}-linux-${ARCHITECTURE}.rpm"

echo "Packaged ${DEB_DIR} and ${RPM_DIR}"
