#!/usr/bin/env bash
set -euo pipefail

PACKAGE="frontend"
DIST_DIR="dist"
DEB_DIR="${DIST_DIR}/deb"
RPM_DIR="${DIST_DIR}/rpm"

usage() {
  cat <<EOF
Usage: $0 [--dist-dir PATH]

Builds Linux release packages (.deb and .rpm).

Defaults:
  Dist dir: ${DIST_DIR}
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
cp target/debian/*.deb "${DEB_DIR}/"
cp target/generate-rpm/*.rpm "${RPM_DIR}/"

echo "Packaged ${DEB_DIR} and ${RPM_DIR}"
